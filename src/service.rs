use anyhow::{Result,ensure};
use serde_json::{Value,json};
use std::{path::Path,collections::HashSet};
use crate::{arr,s,now,id,repository::{self,Texts},security::{digest,redact},storage::Store,history};
#[derive(Clone)]
pub struct ReviewOptions {pub source:String}
impl Default for ReviewOptions {fn default()->Self{Self{source:"all".into()}}}
fn increment(coverage:&mut [Value],agent:&str,key:&str){
    if let Some(row)=coverage.iter_mut().find(|r|r["agent"]==agent){row[key]=json!(row[key].as_u64().unwrap_or(0)+1);}
}
/// Lightweight interactive catalog. Transcripts are imported only when selected.
/// The bounded `review` path remains available for AI evidence gathering.
pub fn workspace_review(path: &Path, opts: &ReviewOptions) -> Result<Value> {
    history::valid_source(&opts.source)?;
    let mut result = review(path, &ReviewOptions { source: "none".into() })?;
    let root = Path::new(s(&result["root"]));
    let (entries, issues) = history::discover_report(root, &opts.source, None, None)?;
    let mut refs: Vec<_> = entries.iter().map(|entry| json!({
        "id":entry["id"], "agent":entry["agent"], "path":entry["path"], "cwd":entry["cwd"],
        "storage_key":format!("source:{}:{}", s(&entry["agent"]), s(&entry["id"])),
        "source_entry":entry, "source_timestamp":entry["timestamp"]
    })).collect();
    let mut seen: HashSet<_> = refs.iter().map(|r| (s(&r["agent"]).to_owned(), s(&r["id"]).to_owned())).collect();
    let (saved_sessions, skipped_headers) = Store::open(root)?.saved_session_catalog()?;
    for saved in saved_sessions {
        if history::source_matches(&opts.source, s(&saved["agent"])) && history::belongs(s(&saved["cwd"]), root)
            && seen.insert((s(&saved["agent"]).to_owned(), s(&saved["id"]).to_owned())) { refs.push(saved); }
    }
    refs.sort_by_key(|r| std::cmp::Reverse(chrono::DateTime::parse_from_rfc3339(
        r["last_event"].as_str().unwrap_or(s(&r["source_timestamp"]))).ok()));
    let coverage: Vec<_> = history::AGENTS.iter().map(|agent| json!({"agent":agent,
        "enabled":history::source_matches(&opts.source, agent),
        "discovered":entries.iter().filter(|entry| entry["agent"] == *agent).count(),
        "captured":0,"skipped_budget":0,"skipped_unreadable":0,"skipped_scope":0,"duplicates":0,"issues":[]
    })).collect();
    result["sessions"] = json!(refs);
    result["coverage"] = json!(coverage);
    result["history_source"] = json!(opts.source);
    result["lazy_sessions"] = json!(true);
    let warnings = result["warnings"].as_array_mut().unwrap();
    warnings.retain(|w| !s(w).starts_with("No agent history captured"));
    warnings.extend(issues.into_iter().map(Value::String));
    if skipped_headers > 0 {
        warnings.push(json!(format!("Skipped {skipped_headers} saved session headers with unsupported or incomplete metadata. Stored snapshots were not modified; available source transcripts can still be rediscovered.")));
    }
    Store::open(Path::new(s(&result["root"])))?.save_review(&result)?;
    Ok(result)
}

/// Compare HEAD with the working tree and capture repository-scoped agent history.
pub fn review(path:&Path,opts:&ReviewOptions)->Result<Value>{
    let root=repository::root(path)?;
    history::valid_source(&opts.source)?;
    let mut store=Store::open(&root)?;let sources=repository::sources(&root)?;let mut warnings=sources.warnings;
    let head=repository::head(&root);
    let before=if let Some(rev)=&head{repository::committed(&root,rev)?}else{Texts::new()};
    let changes=repository::compare(&before,&sources.texts);
    warnings.push("Changes since HEAD may include edits made before the agent session; attribution is unknown.".into());
    let mut coverage=vec![];let mut queues=vec![];
    for agent in history::AGENTS {
        let enabled=history::source_matches(&opts.source,agent);
        let (entries,issues)=if enabled{match history::discover_report(&root,agent,None,None){
            Ok(report)=>report,Err(e)=>(vec![],vec![format!("Discovery failed: {}",redact(&e.to_string()))])
        }}else{(vec![],vec![])};
        warnings.extend(issues.iter().map(|w|format!("{agent}: {w}")));
        coverage.push(json!({"agent":agent,"enabled":enabled,"discovered":entries.len(),"captured":0,
            "skipped_budget":0,"skipped_unreadable":0,"skipped_scope":0,"duplicates":0,"issues":issues}));
        queues.push(entries);
    }
    let mut entries=vec![];
    for i in 0..queues.iter().map(Vec::len).max().unwrap_or(0){for queue in &queues{if let Some(e)=queue.get(i){entries.push(e.clone());}}}
    let mut sessions=vec![];let mut refs=vec![];let mut seen=HashSet::new();let mut total=0;
    for entry in entries{
        let agent=s(&entry["agent"]);let path=Path::new(s(&entry["path"]));
        let Ok(meta)=path.metadata() else {increment(&mut coverage,agent,"skipped_unreadable");warnings.push(format!("Skipped unavailable {agent} session"));continue;};
        let size=if agent=="opencode"{0}else{meta.len()};
        if sessions.len()>=20||total+size>40_000_000||size>20_000_000{increment(&mut coverage,agent,"skipped_budget");continue;}
        let session=match history::collect_entry(&entry){Ok(v)=>v,Err(e)=>{
            increment(&mut coverage,agent,"skipped_unreadable");warnings.push(format!("Skipped unreadable {agent} session: {}",redact(&e.to_string())));continue;
        }};
        if !history::belongs(s(&session["cwd"]),&root)||session["agent"]!=entry["agent"]||session["id"]!=entry["id"]{
            increment(&mut coverage,agent,"skipped_scope");warnings.push(format!("Skipped {agent} session: repository or identity changed during capture"));continue;
        }
        let identity=format!("{}:{}",s(&session["agent"]),s(&session["id"]));
        if seen.contains(&identity){increment(&mut coverage,agent,"duplicates");continue;}
        let size=if agent=="opencode"{session.to_string().len() as u64}else{size};
        if total+size>40_000_000{increment(&mut coverage,agent,"skipped_budget");continue;}
        seen.insert(identity);total+=size;increment(&mut coverage,agent,"captured");
        warnings.extend(arr(&session["warnings"]).iter().map(|w|format!("{}:{}: {}",agent,s(&session["id"]),s(w))));
        let key=format!("{}:{}:{}",agent,s(&session["id"]),&digest(&session.to_string())[..12]);store.put("session",&key,&session)?;
        refs.push(json!({"id":session["id"],"agent":session["agent"],"path":session["path"],"cwd":session["cwd"],"storage_key":key}));sessions.push(session);
    }
    if coverage.iter().any(|r|r["skipped_budget"].as_u64().unwrap_or(0)>0){warnings.push("Some sessions were excluded by the capture budget (20 sessions / 40 MB total / 20 MB per transcript). Use /coverage for counts and /source to narrow capture.".into());}
    if sessions.is_empty(){warnings.push("No agent history captured; explanations use repository evidence only. Use /coverage to inspect discovery.".into());}
    let result=json!({"schema_version":1,"id":id("review"),"root":root,"created_at":now(),"head":head,"baseline_id":null,"session_id":if sessions.len()==1{sessions[0]["id"].clone()}else{Value::Null},"sessions":refs,"coverage":coverage,"recent_code":history::recent_code(&root,&sessions),"decisions":[],"changes":changes.iter().map(|c|repository::changed_file(c,sources.texts.get(&c.file).map(String::as_str).unwrap_or(""))).collect::<Vec<_>>(),"warnings":warnings,"file_hashes":sources.hashes,"input_tokens":0,"output_tokens":0,"provider":"offline","comparison_base":head,"history_source":opts.source});
    store.save_review(&result)?;Ok(result)
}
pub fn load(path:&Path)->Result<Value>{
    let root=repository::root(path)?;let mut result=Store::open(&root)?.get("review","latest")?;crate::validate("Review",&result)?;
    ensure!(s(&result["root"])==root.to_string_lossy(),"Cached review belongs to another repository; press r to refresh");history::saved(&result)?;
    if result["head"]!=json!(repository::head(&root)){result["warnings"].as_array_mut().unwrap().push(json!("This saved review predates the current Git HEAD. Press r to refresh."));}
    Ok(result)
}
