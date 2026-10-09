use anyhow::{Result,ensure};
use serde_json::{Value,json};
use std::{path::{Path,PathBuf},collections::HashSet};
use crate::{arr,s,now,id,repository::{self,Texts},security::{digest,redact},storage::Store,history};
#[derive(Default,Clone)]
pub struct ReviewOptions {pub source:String}
/// Compare HEAD with the working tree and capture repository-scoped agent history.
pub fn review(path:&Path,opts:&ReviewOptions)->Result<Value>{
    let root=repository::root(path)?;
    history::valid_source(&opts.source)?;
    let mut store=Store::open(&root)?;let sources=repository::sources(&root)?;let mut warnings=sources.warnings;
    let head=repository::head(&root);
    let before=if let Some(rev)=&head{repository::committed(&root,rev)?}else{Texts::new()};
    let changes=repository::compare(&before,&sources.texts);
    warnings.push("Changes since HEAD may include edits made before the agent session; attribution is unknown.".into());
    let mut paths=vec![];
    if opts.source!="none"{
        let entries=history::discover(&root,&opts.source,None,None)?;
        let queues:Vec<Vec<_>>=["codex","claude"].iter().map(|a|entries.iter().filter(|e|s(&e["agent"])==*a).collect()).collect();
        for i in 0..queues.iter().map(Vec::len).max().unwrap_or(0){for queue in &queues{if let Some(e)=queue.get(i){paths.push(PathBuf::from(s(&e["path"])));}}}
    }
    let mut sessions=vec![];let mut refs=vec![];let mut seen=HashSet::new();let mut total=0;
    for path in paths{
        let Ok(meta)=path.metadata() else {warnings.push(format!("Skipped unavailable session: {}",path.display()));continue;};
        let size=meta.len();
        if sessions.len()>=20||total+size>40_000_000||size>20_000_000{warnings.push(format!("Skipped session due to history budget (20 sessions / 40 MB total): {}",path.display()));continue;}
        let session=match history::collect(&path){Ok(v)=>v,Err(e)=>{warnings.push(format!("Skipped unreadable session {}: {}",path.display(),redact(&e.to_string())));continue;}};
        if !history::belongs(s(&session["cwd"]),&root){continue;}
        if !seen.insert(format!("{}:{}",s(&session["agent"]),s(&session["id"]))){continue;}
        total+=size;warnings.extend(arr(&session["warnings"]).iter().map(|w|format!("{}:{}: {}",s(&session["agent"]),s(&session["id"]),s(w))));
        let key=format!("{}:{}:{}",s(&session["agent"]),s(&session["id"]),&digest(&session.to_string())[..12]);store.put("session",&key,&session)?;
        refs.push(json!({"id":session["id"],"agent":session["agent"],"path":session["path"],"cwd":session["cwd"],"storage_key":key}));sessions.push(session);
    }
    if sessions.is_empty(){warnings.push("No agent history supplied; explanations use repository evidence only.".into());}
    let result=json!({"schema_version":1,"id":id("review"),"root":root,"created_at":now(),"head":head,"baseline_id":null,"session_id":if sessions.len()==1{sessions[0]["id"].clone()}else{Value::Null},"sessions":refs,"recent_code":history::recent_code(&root,&sessions),"decisions":[],"changes":changes.iter().map(|c|repository::changed_file(c,sources.texts.get(&c.file).map(String::as_str).unwrap_or(""))).collect::<Vec<_>>(),"warnings":warnings,"file_hashes":sources.hashes,"input_tokens":0,"output_tokens":0,"provider":"offline","comparison_base":head,"history_source":opts.source});
    store.save_review(&result)?;Ok(result)
}
pub fn load(path:&Path)->Result<Value>{
    let root=repository::root(path)?;let mut result=Store::open(&root)?.get("review","latest")?;crate::validate("Review",&result)?;
    ensure!(s(&result["root"])==root.to_string_lossy(),"Cached review belongs to another repository; press r to refresh");history::saved(&result)?;
    if result["head"]!=json!(repository::head(&root)){result["warnings"].as_array_mut().unwrap().push(json!("This saved review predates the current Git HEAD. Press r to refresh."));}
    Ok(result)
}
