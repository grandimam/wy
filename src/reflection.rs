use anyhow::{Result,ensure};
use serde_json::{Value,json};
use std::{path::Path,collections::{BTreeMap,HashSet}};
use crate::{s,n,arr,id,now,service,repository,storage::Store,engine::code_evidence,security::{digest,redact,clean},provider::validate_citations};
fn without_assessments(d:&Value)->Value{let mut d=d.clone();if let Some(o)=d.as_object_mut(){o.remove("reflections");o.remove("stale");}d}
// Stable sorted JSON spelling preserves pending reflection fingerprints.
fn canonical_json(v:&Value)->String{match v{
    Value::Array(xs)=>format!("[{}]",xs.iter().map(canonical_json).collect::<Vec<_>>().join(", ")),
    Value::Object(xs)=>format!("{{{}}}",xs.iter().map(|(k,v)|format!("{}: {}",canonical_json(&json!(k)),canonical_json(v))).collect::<Vec<_>>().join(", ")),
    Value::String(_)=>v.to_string().chars().map(|c|if c as u32>127{let mut buf=[0;2];c.encode_utf16(&mut buf).iter().map(|u|format!("\\u{u:04x}")).collect::<String>()}else{c.to_string()}).collect(),_=>v.to_string()}}
pub fn fingerprint(d:&Value)->String{digest(&canonical_json(&without_assessments(d)))}
pub fn focus(root:&Path,target:&str,question:&str,evidence:&[String])->Result<Value>{
    let mut review=service::load(root)?;let root=Path::new(s(&review["root"])).to_path_buf();let sources=repository::sources(&root)?;
    let resolve=|target:&str|->Result<(String,usize)>{let(file,line)=target.rsplit_once(':').ok_or_else(||anyhow::anyhow!("Use a repository-relative file:line"))?;let line=line.parse::<usize>().map_err(|_|anyhow::anyhow!("Use a repository-relative file:line"))?;
        ensure!(sources.texts.get(file).is_some_and(|t|line>0&&line<=t.lines().count()),"Location must refer to an eligible source file and existing line");ensure!(sources.hashes.get(file).map(String::as_str)==review["file_hashes"][file].as_str(),"Source changed since review; run wy review before adding a question");Ok((file.into(),line))};
    let(file,line)=resolve(target)?;let question=redact(question.trim());ensure!(!question.is_empty()&&question.chars().count()<=500,"Question must contain 1 to 500 characters");ensure!(evidence.len()<=11,"Supply at most 11 additional evidence locations");
    let mut locations=vec![(file.clone(),line)];for e in evidence{let l=resolve(e)?;if !locations.contains(&l){locations.push(l);}}
    let mut loc=repository::location(&file,&sources.texts[&file],line);loc["start_line"]=json!(line);loc["end_line"]=json!(n(&loc["end_line"]).min(line+3));
    let d=json!({"id":format!("decision-{}",&digest(&format!("focus:{file}:{line}:{question}"))[..12]),"question":question,"category":"architecture","location":loc,"explanation":"This question was explicitly nominated; original intent has not been established.","provenance":"unexplained","evidence":locations.iter().map(|(f,n)|code_evidence(f,&sources.texts[f],*n,&sources.hashes)).collect::<Vec<_>>(),"snapshot_hash":sources.hashes[&file],"attribution":"unknown","stale":false,"reflections":[],"alternatives":[],"assumptions":[],"unresolved_questions":[]});
    ensure!(!arr(&review["decisions"]).iter().any(|other|other["id"]==d["id"]),"This question already exists; use wy explain or reflection-request");ensure!(arr(&review["decisions"]).len()<100,"Review already contains 100 decisions; start a more focused review");
    review["decisions"].as_array_mut().unwrap().push(d.clone());Store::open(&root)?.save_review(&review)?;Ok(d)
}
pub fn request(root:&Path,target:Option<&str>)->Result<Value>{
    let review=service::load(root)?;let decisions=if let Some(t)=target{vec![service::select(&review,t)?.clone()]}else{arr(&review["decisions"]).to_vec()};
    ensure!(!decisions.is_empty(),"No decisions to reflect on; run wy review first");ensure!(decisions.len()<=12,"Select a decision ID or file:line for reviews with more than 12 decisions");ensure!(decisions.iter().all(|d|d["stale"]!=true),"Decision or evidence has changed; run wy review before reflection");
    let result=json!({"request_id":id("reflection"),"review_id":review["id"],"source_session_id":review["session_id"],"source_sessions":review["sessions"],"instructions":crate::reasoning::prompt("reflection"),"decisions":decisions.iter().map(without_assessments).collect::<Vec<_>>(),"response_schema":crate::schema("ReflectionResponse")});
    let fingerprints:BTreeMap<_,_>=decisions.iter().map(|d|(s(&d["id"]),fingerprint(d))).collect();
    Store::open(Path::new(s(&review["root"])))?.put("reflection-request",s(&result["request_id"]),&json!({"review_id":review["id"],"fingerprints":fingerprints}))?;Ok(result)
}
pub fn record(root:&Path,mut response:Value)->Result<Value>{
    clean(&mut response);crate::validate("ReflectionResponse",&response)?;let mut review=service::load(root)?;let mut store=Store::open(Path::new(s(&review["root"])))?;
    let pending=store.get("reflection-request",s(&response["request_id"]))?;
    ensure!(response["review_id"]==review["id"]&&pending["review_id"]==review["id"],"Reflection targets a different review; create a new reflection request");
    let expected=pending["fingerprints"].as_object().ok_or_else(||anyhow::anyhow!("Invalid reflection request"))?;
    let ids:HashSet<_>=arr(&response["reflections"]).iter().map(|r|s(&r["decision_id"])).collect();
    ensure!(ids.len()==arr(&response["reflections"]).len()&&ids.len()==expected.len()&&ids.iter().all(|id|expected.contains_key(*id)),"Return every requested decision exactly once");
    for item in arr(&response["reflections"]){
        let d=service::select(&review,s(&item["decision_id"]))?;
        ensure!(d["stale"]!=true&&json!(fingerprint(d))==expected[s(&item["decision_id"])],"Decision or evidence has changed; create a fresh review and request");
        ensure!(!arr(&d["reflections"]).iter().any(|r|r["request_id"]==response["request_id"]),"Reflection request has already been recorded");
        validate_citations(&item["evidence_ids"],d)?;ensure!(item["assessment"]=="insufficient_context"||!arr(&item["evidence_ids"]).is_empty(),"Keep/revise assessments require supporting citations");
    }
    let created=now();for item in arr(&response["reflections"]){let mut r=item.clone();for key in ["request_id","agent","model","context"]{r[key]=response[key].clone();}r["created_at"]=json!(created);r["identity_verification"]=json!("self_reported");for d in review["decisions"].as_array_mut().unwrap(){if d["id"]==item["decision_id"]{d["reflections"].as_array_mut().unwrap().push(r.clone());}}}
    store.save_review(&review)?;Ok(review)
}
