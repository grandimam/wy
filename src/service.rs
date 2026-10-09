use anyhow::{Result,ensure,bail};
use serde_json::{Value,json};
use std::{path::{Path,PathBuf},collections::{HashSet,BTreeMap}};
use crate::{arr,s,n,now,id,repository::{self,Texts},security::{digest,redact,short,read_source},storage::Store,history,engine,provider::Provider};
#[derive(Default,Clone)]
pub struct ReviewOptions {pub baseline:Option<String>,pub base:Option<String>,pub diff:Option<PathBuf>,pub sessions:Vec<PathBuf>,pub source:String,pub model:bool}
pub fn snapshot(path:&Path)->Result<Value>{
    let root=repository::root(path)?;let sources=repository::sources(&root)?;
    ensure!(!sources.warnings.iter().any(|w|w.contains("truncated")),"Cannot establish a complete baseline: repository context exceeds 12 MB");
    let data=json!({"id":id("snapshot"),"root":root,"head":repository::head(&root),"created_at":now(),"texts":sources.texts,"hashes":sources.hashes,"warnings":sources.warnings});
    Store::open(&root)?.put("snapshot",s(&data["id"]),&data)?;Ok(data)
}
pub fn review(path:&Path,opts:&ReviewOptions)->Result<Value>{
    let root=repository::root(path)?;
    ensure!(usize::from(opts.baseline.is_some())+usize::from(opts.base.is_some())+usize::from(opts.diff.is_some())<=1,"Choose only one of --baseline, --base or --diff");
    history::valid_source(&opts.source)?;
    let mut store=Store::open(&root)?;let sources=repository::sources(&root)?;let mut warnings=sources.warnings;
    let changes=if let Some(base)=&opts.baseline{
        let snapshot=store.get("snapshot",base)?;ensure!(s(&snapshot["root"])==root.to_string_lossy(),"Baseline belongs to another repository");
        warnings.push("Changes since the baseline are isolated, but authorship is not proven; concurrent human edits may be included.".into());
        let texts:Texts=serde_json::from_value(snapshot["texts"].clone())?;repository::compare(&texts,&sources.texts)
    }else if let Some(path)=&opts.diff{
        ensure!(path.metadata()?.len()<=5_000_000,"Diff exceeds the 5 MB input limit");
        let changes=repository::parse_diff(&std::fs::read_to_string(path)?);
        for c in &changes{let lines:Vec<_>=sources.texts.get(&c.file).map(String::as_str).unwrap_or("").lines().collect();ensure!(c.additions.iter().all(|(n,t)|*n>0&&lines.get(n-1).is_some_and(|l|*l==t)),"Diff does not match current source: {}; check out its target snapshot",c.file);}
        changes
    }else{
        let rev=opts.base.clone().or_else(||repository::head(&root));let before=if let Some(rev)=rev{repository::committed(&root,&rev)?}else{Texts::new()};repository::compare(&before,&sources.texts)
    };
    if opts.baseline.is_none(){warnings.push("No pre-session baseline: existing uncommitted changes cannot be distinguished from agent changes; attribution is unknown.".into());}
    let automatic=opts.sessions.is_empty()&&opts.source!="none";let mut paths=opts.sessions.clone();
    if automatic{
        let entries=history::discover(&root,&opts.source,None,None)?;
        let queues:Vec<Vec<_>>=["codex","claude"].iter().map(|a|entries.iter().filter(|e|s(&e["agent"])==*a).collect()).collect();
        for i in 0..queues.iter().map(Vec::len).max().unwrap_or(0){for queue in &queues{if let Some(e)=queue.get(i){paths.push(PathBuf::from(s(&e["path"])));}}}
    }else{ensure!(paths.len()<=20,"Select at most 20 sessions per review");}
    let mut sessions=vec![];let mut refs=vec![];let mut seen=HashSet::new();let mut total=0;
    for path in paths{
        let size=match path.metadata(){Ok(m)=>m.len(),Err(_) if automatic=>{warnings.push(format!("Skipped unavailable session: {}",path.display()));continue;},Err(e)=>return Err(e.into())};
        if automatic&&(sessions.len()>=20||total+size>40_000_000||size>20_000_000){warnings.push(format!("Skipped session due to history budget (20 sessions / 40 MB total): {}",path.display()));continue;}
        ensure!(total+size<=40_000_000,"Selected histories exceed the 40 MB total input limit");
        let session=match history::collect(&path){Ok(v)=>v,Err(e) if automatic=>{warnings.push(format!("Skipped unreadable session {}: {}",path.display(),redact(&e.to_string())));continue;},Err(e)=>return Err(e)};
        if !history::belongs(s(&session["cwd"]),&root){if automatic{continue;}bail!("Session working directory does not match this repository or is missing; refusing unrelated or unverified history");}
        ensure!(!["codex","claude"].contains(&opts.source.as_str())||session["agent"]==opts.source,"Explicit session does not match the selected history source");
        if !seen.insert(format!("{}:{}",s(&session["agent"]),s(&session["id"]))){continue;}
        total+=size;warnings.extend(arr(&session["warnings"]).iter().map(|w|format!("{}:{}: {}",s(&session["agent"]),s(&session["id"]),s(w))));
        let key=format!("{}:{}:{}",s(&session["agent"]),s(&session["id"]),&digest(&session.to_string())[..12]);store.put("session",&key,&session)?;
        refs.push(json!({"id":session["id"],"agent":session["agent"],"path":session["path"],"cwd":session["cwd"],"storage_key":key}));sessions.push(session);
    }
    if sessions.is_empty(){warnings.push("No agent history supplied; explanations use repository evidence only.".into());}
    let mut decisions=engine::analyze(&changes,&sources.texts,&sources.hashes,&sessions,opts.baseline.is_some());
    if decisions.len()==12{warnings.push("Annotation limit reached (12); lower-priority candidates may be omitted.".into());}
    let mut input=0;let mut output=0;
    if opts.model{let mut provider=Provider::from_env()?;for decision in &mut decisions{if let Err(_e)=provider.enrich(decision){warnings.push(format!("Invalid or unavailable model result for {}; kept conservative offline analysis.",s(&decision["id"])));}}input=provider.input;output=provider.output;}
    if decisions.is_empty(){warnings.push("No supported significant-decision patterns found; this does not mean the change has no engineering decisions.".into());}
    let comparison=opts.baseline.clone().or_else(||opts.diff.as_ref().map(|p|format!("imported patch: {}",p.display()))).or_else(||opts.base.clone()).or_else(||repository::head(&root));
    let result=json!({"schema_version":1,"id":id("review"),"root":root,"created_at":now(),"head":repository::head(&root),"baseline_id":opts.baseline,"session_id":if sessions.len()==1{sessions[0]["id"].clone()}else{Value::Null},"sessions":refs,"recent_code":history::recent_code(&root,&sessions),"decisions":decisions,"changes":changes.iter().map(|c|repository::changed_file(c,sources.texts.get(&c.file).map(String::as_str).unwrap_or(""))).collect::<Vec<_>>(),"warnings":warnings,"file_hashes":sources.hashes,"input_tokens":input,"output_tokens":output,"provider":if opts.model{"ollama"}else{"offline"},"comparison_base":comparison,"history_source":if !opts.sessions.is_empty(){"selected"}else{&opts.source}});
    store.save_review(&result)?;Ok(result)
}
pub fn fresh(decision:&Value,root:&Path)->bool{
    let mut files=BTreeMap::from([(s(&decision["location"]["file"]),s(&decision["snapshot_hash"]))]);
    for e in arr(&decision["evidence"]).iter().filter(|e|e["kind"]=="code"){files.insert(s(&e["file"]),s(&e["snapshot_hash"]));}
    files.into_iter().all(|(f,h)|!h.is_empty()&&read_source(root,f).is_some_and(|t|digest(&t)==h))
}
pub fn load(path:&Path)->Result<Value>{
    let root=repository::root(path)?;let mut result=Store::open(&root)?.get("review","latest")?;crate::validate("Review",&result)?;
    ensure!(s(&result["root"])==root.to_string_lossy(),"Cached review belongs to another repository; run wy review again");history::saved(&result)?;
    if result["head"]!=json!(repository::head(&root)){result["warnings"].as_array_mut().unwrap().push(json!("This saved review predates the current Git HEAD. Run wy review or wy reason to inspect current changes."));}
    for d in result["decisions"].as_array_mut().unwrap(){d["stale"]=json!(!fresh(d,&root));}Ok(result)
}
pub fn select<'a>(review:&'a Value,target:&str)->Result<&'a Value>{
    let decisions=arr(&review["decisions"]);
    if let Ok(i)=target.parse::<usize>(){return decisions.get(i.wrapping_sub(1)).ok_or_else(||anyhow::anyhow!("Decision number out of range; run wy decisions for the current index"));}
    if let Some(d)=decisions.iter().find(|d|d["id"]==target){return Ok(d);}
    let (file,line)=target.rsplit_once(':').ok_or_else(||anyhow::anyhow!("Use a decision number, decision ID or file:line"))?;
    let line:usize=line.parse().map_err(|_|anyhow::anyhow!("Use a decision number, decision ID or file:line"))?;
    let file=if Path::new(file).is_absolute(){Path::new(file).strip_prefix(s(&review["root"]))?.to_string_lossy().into_owned()}else{file.into()};
    decisions.iter().find(|d|d["location"]["file"]==file&&n(&d["location"]["start_line"])<=line&&line<=n(&d["location"]["end_line"])).ok_or_else(||anyhow::anyhow!("No decision at this location; run wy decisions to see available locations"))
}
pub fn ask(decision:&Value,question:&str,model:bool)->Result<Value>{
    ensure!(decision["stale"]!=true,"Decision or evidence has changed; run wy review before asking follow-up questions");
    if model{return Provider::from_env()?.ask(decision,question);}
    let q=question.to_lowercase();let strings=|v:&Value|arr(v).iter().map(s).collect::<Vec<_>>().join("\n");
    let answer=if q.contains("alternative"){format!("Options to investigate (not a record of what the agent considered):\n{}",strings(&decision["alternatives"]))}
    else if ["assum","verify","uncertain","gap"].iter().any(|w|q.contains(w)){format!("{}\n{}",strings(&decision["assumptions"]),strings(&decision["unresolved_questions"]))}
    else if q.contains("evidence")||q.contains("support"){arr(&decision["evidence"]).iter().map(|e|format!("[{}] {}:{}\n{}",s(&e["id"]),s(&e["file"]),n(&e["start_line"]),s(&e["excerpt"]))).collect::<Vec<_>>().join("\n\n")}
    else if q.contains("convention")||q.contains("consistent"){format!("The retrieved examples are not sufficient to establish a repository-wide convention.\n{}",strings(&decision["unresolved_questions"]))}
    else if ["why","selected","approach"].iter().any(|w|q.contains(w)){s(&decision["explanation"]).into()}
    else{"Offline investigation supports why, alternatives, evidence, conventions and assumptions. Use --model for a semantic follow-up.".into()};
    Ok(json!({"answer":short(&answer,5000),"evidence_ids":arr(&decision["evidence"]).iter().map(|e|e["id"].clone()).collect::<Vec<_>>(),"uncertainty":format!("Offline response uses only the cached review. {}",if decision["provenance"]=="recorded"{"Recorded rationale is an assistant statement, not verified correctness."}else{"Original intent is not established."})}))
}
