//! Normalize public transcript data without executing agents or modifying their stores.
use super::*;
use rusqlite::{Connection, OpenFlags};

fn stamp(ms:i64)->String {chrono::DateTime::from_timestamp_millis(ms).map(|t|t.to_rfc3339_opts(chrono::SecondsFormat::Millis,true)).unwrap_or_default()}
fn open(path:&Path)->Result<Connection>{
    let db=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_ONLY|OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    db.busy_timeout(std::time::Duration::from_secs(2))?;Ok(db)
}
pub(super) fn discover_opencode(root:&Path)->(Vec<Value>,Vec<String>){
    let dir=home("XDG_DATA_HOME",".local/share").join("opencode");
    let mut result=vec![];let mut warnings=vec![];
    if dir.join("storage/session").exists(){warnings.push("OpenCode legacy JSON storage detected; this layout is not supported".into());}
    for name in ["opencode.db","opencode-dev.db"] {
        let path=dir.join(name);if !path.try_exists().unwrap_or(true){continue;}
        match discover_db(root,&path){Ok(entries)=>result.extend(entries),Err(_)=>warnings.push(format!("OpenCode {name} could not be read; database may be busy, inaccessible, or use an unsupported schema"))}
    }
    (result,warnings)
}
fn discover_db(root:&Path,path:&Path)->Result<Vec<Value>>{
    let db=open(path)?;
    let mut query=db.prepare("SELECT id,directory,time_created FROM session ORDER BY time_created DESC")?;
    let rows=query.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?)))?;
    let mut result=vec![];
    for row in rows {let(id,cwd,time)=row?;if belongs(&cwd,root){result.push(json!({"id":id,"cwd":cwd,"timestamp":stamp(time),"path":path.canonicalize()?,"agent":"opencode"}));}}
    Ok(result)
}
/// A database path alone is not a session identifier; retain the discovered entry.
pub fn collect_entry(entry:&Value)->Result<Value>{
    let path=Path::new(s(&entry["path"]));
    if entry["agent"]=="opencode" {opencode(path,s(&entry["id"]))}else{super::collect(path)}
}
fn tool(name:&str,input:&Value,id:&Value)->Value{
    let mut input=if input.is_object(){input.clone()}else{json!({})};
    if let Some(f)=input.get("path").or_else(||input.get("filePath")).cloned(){input["file_path"]=f;}
    let changes=|v:&mut Value|{
        for (target,keys) in [("old_string",["oldText","oldString"]),("new_string",["newText","newString"])] {
            if let Some(value)=keys.iter().find_map(|k|v.get(*k)).cloned(){v[target]=value;}
        }
    };
    changes(&mut input);
    if let Some(edits)=input["edits"].as_array_mut(){for edit in edits{changes(edit);}}
    if let Some(patch)=input.get("patchText").cloned(){input["patch"]=patch;}
    let name=match name {"write"=>"Write","edit" if input["edits"].is_array()=>"MultiEdit","edit"=>"Edit","read"=>"Read","grep"=>"Grep","glob"|"find"=>"Glob",other=>other};
    json!({"type":"tool_use","name":name,"input":input,"id":id})
}
fn row(header:&Value,id:&Value,time:&Value,role:&str,content:Value,model:&Value)->Value{
    json!({"type":role,"sessionId":header["id"],"cwd":header["cwd"],"uuid":id,"timestamp":time,"message":{"model":model,"content":content}})
}
fn finish(path:&Path,agent:&str,header:&Value,rows:Vec<Value>,warnings:Vec<String>)->Result<Value>{
    // Include a metadata-only row for empty sessions.
    let mut raw=row(header,&json!("header"),&header["timestamp"],"user",json!([]),&Value::Null).to_string();
    for r in rows {raw.push('\n');raw.push_str(&r.to_string());ensure!(raw.len()<=20_000_000,"Session exceeds the 20 MB input limit");}
    let mut session=super::collect_raw(path,&raw)?;
    session["agent"]=json!(agent);session["format"]=json!(if agent=="pi"{"pi-jsonl"}else{"opencode-sqlite"});
    session["warnings"].as_array_mut().unwrap().extend(warnings.into_iter().map(Value::String));
    crate::validate("Session",&session)?;Ok(session)
}
pub(super) fn pi(path:&Path,raw:&str)->Result<Value>{
    let mut lines=raw.lines();let header:Value=serde_json::from_str(lines.next().unwrap_or(""))?;
    ensure!(header["id"].is_string()&&header["cwd"].is_string(),"Invalid pi session header");
    let lines:Vec<_>=lines.collect();
    let entries:Vec<Value>=lines.iter().filter_map(|l|serde_json::from_str(l).ok()).collect();
    let tree:HashMap<_,_>=entries.iter().filter_map(|r|r["id"].as_str().map(|id|(id,r))).collect();
    let branched=entries.iter().any(|r|r.get("parentId").is_some());
    let mut active=HashSet::new();let mut leaf=entries.last().and_then(|r|r["id"].as_str());
    while let Some(id)=leaf {ensure!(active.insert(id),"Cycle in pi session tree");leaf=tree.get(id).and_then(|r|r["parentId"].as_str());}
    let mut rows=vec![];let mut warnings=vec![];
    for (i,line) in lines.iter().enumerate(){
        if branched && serde_json::from_str::<Value>(line).ok().is_some_and(|r|!active.contains(s(&r["id"]))){
            rows.push(row(&header,&json!(format!("ignored-{i}")),&Value::Null,"assistant",json!([]),&Value::Null));continue;
        }
        let r:Value=match serde_json::from_str(line){Ok(r)=>r,Err(_)=>{warnings.push(format!("Skipped malformed pi line {}",i+2));rows.push(row(&header,&json!(format!("malformed-{i}")),&Value::Null,"assistant",json!([]),&Value::Null));continue;}};
        let m=&r["message"];let role=s(&m["role"]);let mut blocks=vec![];
        match s(&r["type"]){
            "compaction"|"branch_summary"=>blocks.push(json!({"type":"compaction","content":r["summary"]})),
            "message"=>match role {
                "user"|"assistant"=>{
                    if m["content"].is_string(){blocks.push(json!({"type":"text","text":m["content"]}));}
                    for b in arr(&m["content"]){match s(&b["type"]){
                        "text"=>blocks.push(json!({"type":"text","text":b["text"]})),
                        "thinking"=>blocks.push(json!({"type":"thinking","thinking":b["thinking"]})), 
                        "toolCall"=>blocks.push(tool(s(&b["name"]),&b["arguments"],&b["id"])),_=>{}}
                    }
                },
                "toolResult"=>blocks.push(json!({"type":"tool_result","tool_use_id":m["toolCallId"],"content":m["content"],"is_error":m["isError"]})),
                _=>{}
            },_=>{}
        }
        let role=if role=="user"||role=="toolResult"{"user"}else{"assistant"};
        // Keep one normalized row per original entry, including ignored metadata.
        rows.push(row(&header,&r["id"],&r["timestamp"],role,json!(blocks),&m["model"]));
    }
    if branched {warnings.push("Pi capture follows the last recorded branch; abandoned branches are excluded.".into());}
    finish(path,"pi",&header,rows,warnings)
}
fn opencode(path:&Path,id:&str)->Result<Value>{
    let mut db=open(path)?;let tx=db.transaction()?;
    let (cwd,time):(String,i64)=tx.query_row("SELECT directory,time_created FROM session WHERE id=?",[id],|r|Ok((r.get(0)?,r.get(1)?)))?;
    let header=json!({"id":id,"cwd":cwd,"timestamp":stamp(time)});
    let mut query=tx.prepare("SELECT p.id,p.time_created,m.data,p.data FROM part p JOIN message m ON p.message_id=m.id AND p.session_id=m.session_id WHERE p.session_id=? ORDER BY m.time_created,m.id,p.time_created,p.id")?;
    let items=query.query_map([id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?)))?;
    let mut rows=vec![];let mut bytes=0;
    for item in items {
        let (part,time,message,data)=item?;bytes+=message.len()+data.len();ensure!(bytes<=20_000_000,"Session exceeds the 20 MB input limit");
        let m:Value=serde_json::from_str(&message)?;let p:Value=serde_json::from_str(&data)?;
        let role=s(&m["role"]);if !["user","assistant"].contains(&role){continue;}
        let model=m.get("modelID").unwrap_or(&Value::Null);let time=json!(stamp(time));let mut blocks=vec![];
        match s(&p["type"]){
            "text"=>blocks.push(if m["summary"]==true||p["synthetic"]==true{json!({"type":"compaction","content":p["text"]})}else{json!({"type":"text","text":p["text"]})}),
            "reasoning"=>blocks.push(json!({"type":"thinking","thinking":p["text"]})),
            "tool"=>{
                blocks.push(tool(s(&p["tool"]),&p["state"]["input"],&p["callID"]));
                rows.push(row(&header,&json!(part),&time,"assistant",json!(blocks),model));blocks=vec![];
                if ["completed","error"].contains(&s(&p["state"]["status"])){
                    blocks.push(json!({"type":"tool_result","tool_use_id":p["callID"],"is_error":p["state"]["status"]=="error","content":p["state"].get("output").or_else(||p["state"].get("error")).unwrap_or(&Value::Null)}));
                    rows.push(row(&header,&json!(format!("{part}-result")),&time,"user",json!(blocks),model));
                }
                continue;
            },_=>{}
        }
        rows.push(row(&header,&json!(part),&time,role,json!(blocks),model));
    }
    finish(path,"opencode",&header,rows,vec!["OpenCode source lines are normalized event positions, not database line numbers.".into()])
}
