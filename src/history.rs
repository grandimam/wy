mod notes;
pub use notes::{notes, note_evidence, event_evidence};
mod edits;
mod recent;
pub use recent::{recent_code, saved_edit, edit_ref};
use anyhow::{Result,ensure,bail};
use serde_json::{json,Value};
use std::{collections::{HashSet,HashMap},fs::File,io::{BufRead,BufReader,Read},path::{Path,PathBuf}};
use crate::{arr,s,security::{redact,short},repository,storage::Store};

pub fn valid_source(source:&str)->Result<()> {ensure!(["both","codex","claude","none"].contains(&source),"History source must be codex, claude, both or none");Ok(())}
pub fn belongs(cwd:&str,root:&Path)->bool{
    if !Path::new(cwd).is_absolute(){return false;}
    let Ok(path)=Path::new(cwd).canonicalize() else{return false};
    path==root || path.starts_with(root)&&repository::root(&path).is_ok_and(|p|p==root)
}
fn home(env:&str,fallback:&str)->PathBuf {std::env::var_os(env).map(PathBuf::from).unwrap_or_else(||PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(fallback))}
fn metadata(path:&Path,agent:&str)->Option<Value>{
    let limit=if agent=="codex"{128000}else{1_000_000};
    let mut reader=BufReader::new(File::open(path).ok()?.take(limit));
    let mut line=String::new();
    for _ in 0..if agent=="codex"{1}else{100}{
        line.clear();if reader.read_line(&mut line).ok()?==0{break;}
        let Ok(row)=serde_json::from_str::<Value>(&line) else{continue};
        if agent=="codex"&&row["type"]=="session_meta"{
            let p=&row["payload"];let id=p.get("id").or_else(||p.get("session_id")).cloned().unwrap_or(json!(path.file_stem()?.to_string_lossy()));
            return Some(json!({"id":id,"path":path.canonicalize().ok()?,"cwd":p["cwd"],"timestamp":p["timestamp"],"agent":agent}));
        }
        if agent=="claude"&&row["cwd"].is_string()&&row["sessionId"].is_string(){return Some(json!({"id":row["sessionId"],"path":path.canonicalize().ok()?,"cwd":row["cwd"],"timestamp":row["timestamp"],"agent":agent}));}
    }None
}
pub fn discover(root:&Path,source:&str,codex_home:Option<&Path>,claude_home:Option<&Path>)->Result<Vec<Value>>{
    valid_source(source)?;let root=repository::root(root)?;let mut entries=vec![];let mut seen=HashSet::new();let mut checked=HashMap::new();
    for (agent,default,override_home) in [("codex",home("CODEX_HOME",".codex"),codex_home),("claude",home("CLAUDE_CONFIG_DIR",".claude"),claude_home)]{
        if source!="both"&&source!=agent{continue;}
        for home in [override_home.unwrap_or(&default).to_path_buf(),root.join(format!(".{agent}"))]{
            let dirs=if agent=="codex"{vec![home.join("sessions"),home.join("archived_sessions")]}else{vec![home.join("projects")]};
            for dir in dirs{for entry in walkdir::WalkDir::new(dir).max_depth(if agent=="claude"{2}else{64}).follow_links(false).sort_by_file_name().into_iter().filter_map(Result::ok){
                if !entry.file_type().is_file()||entry.path().extension().is_none_or(|s|s!="jsonl"){continue;}
                if let Some(item)=metadata(entry.path(),agent){
                    let cwd=s(&item["cwd"]).to_owned();let matches=*checked.entry(cwd.clone()).or_insert_with(||belongs(&cwd,&root));
                    if matches&&seen.insert(format!("{agent}:{}",s(&item["id"]))){entries.push(item);}
                }
            }}
        }
    }
    entries.sort_by(|a,b|s(&b["timestamp"]).cmp(s(&a["timestamp"])));Ok(entries)
}
pub fn resolve(root:&Path,selectors:&[String],source:&str)->Result<Vec<PathBuf>>{
    let mut entries=None;let mut paths=vec![];
    for selector in selectors{
        if Path::new(selector).is_file(){paths.push(PathBuf::from(selector));continue;}
        if entries.is_none(){entries=Some(discover(root,source,None,None)?);}
        let matches:Vec<_>=entries.as_ref().unwrap().iter().filter(|e|selector==s(&e["id"])||*selector==format!("{}:{}",s(&e["agent"]),s(&e["id"]))).collect();
        ensure!(matches.len()==1,"Session not found in this repository or ambiguous; use agent:id or an explicit path");paths.push(PathBuf::from(s(&matches[0]["path"])));
    }Ok(paths)
}
fn visible(value:&Value)->String{
    if let Some(t)=value.as_str(){return t.into();}
    if value.is_array(){return arr(value).iter().filter(|b|["text","input_text","output_text"].contains(&s(&b["type"]))).map(|b|s(&b["text"])).collect::<Vec<_>>().join("\n");}
    value.to_string()
}
fn kind(name:&str,text:&str)->&'static str{
    if name.contains("apply_patch")||["Edit","MultiEdit","Write"].contains(&name){"change"}
    else if name=="Read"{"read"}else if ["Glob","Grep"].contains(&name){"search"}
    else if regex::Regex::new(r"\b(cargo test|npm test|npm run test)\b").unwrap().is_match(text){"test"}
    else if regex::Regex::new(r"\b(rg|grep|find)\b").unwrap().is_match(text){"search"}
    else if regex::Regex::new(r"\b(cat|sed|head|read_file)\b").unwrap().is_match(text){"read"}else{"tool_call"}
}
pub fn collect(path:&Path)->Result<Value>{
    ensure!(path.metadata()?.len()<=20_000_000,"Session exceeds the 20 MB input limit");
    let raw=std::fs::read_to_string(path)?;let mut agent="";
    for line in short(&raw,1_000_000).lines().take(100){
        let Ok(row)=serde_json::from_str::<Value>(line)else{continue};
        if ["session_meta","thread.started","response_item","event_msg","item.completed"].contains(&s(&row["type"])){agent="codex";break;}
        if row["sessionId"].is_string()&&["user","assistant","progress"].contains(&s(&row["type"])){agent="claude";break;}
    }
    ensure!(!agent.is_empty(),"Unrecognized session format; expected a Codex or Claude Code JSONL transcript");
    let mut session=json!({"id":path.file_stem().unwrap_or_default().to_string_lossy(),"path":path.canonicalize()?,"cwd":null,"agent":agent,"format":if agent=="codex"{"codex-rollout"}else{"claude-code-jsonl"},"events":[],"warnings":[]});
    let mut events=vec![];let mut warnings=vec![];let mut seen=HashSet::new();
    for (i,line) in raw.lines().enumerate(){
        let row=match serde_json::from_str::<Value>(line){Ok(v) if v.is_object()=>v,_=>{warnings.push(format!("Skipped malformed session line {}",i+1));continue;}};
        let typ=s(&row["type"]);let p=&row["payload"];
        let (cwd,identity)=if agent=="claude"{(&row["cwd"],&row["sessionId"])}else{(&p["cwd"],if typ=="session_meta"{p.get("id").or_else(||p.get("session_id")).unwrap_or(&Value::Null)}else{&Value::Null})};
        if cwd.is_string(){ensure!(session["cwd"].is_null()||session["cwd"]==*cwd,"Session contains conflicting working directories");session["cwd"]=cwd.clone();}
        if identity.is_string(){ensure!(events.is_empty()||session["id"]==*identity,"Session contains conflicting session identities");session["id"]=identity.clone();}
        let mut pending:Vec<(String,String,Value,Value,Vec<String>,String,bool)>=vec![];
        if agent=="claude" {
            if !["user","assistant"].contains(&typ){continue;}
            let content=&row["message"]["content"];
            let blocks=if content.is_string(){vec![json!({"type":"text","text":content})]}else{arr(content).to_vec()};
            for (b,block) in blocks.iter().enumerate(){
                let mut tool=Value::Null;let mut call=Value::Null;let mut files=vec![];
                let (k,t)=match s(&block["type"]){
                    "text"=>(typ,s(&block["text"]).to_owned()),
                    "tool_use" if typ=="assistant"=>{tool=block["name"].clone();call=block["id"].clone();if let Some(f)=block["input"]["file_path"].as_str(){files.push(f.into());}let t=block["input"].to_string();(kind(s(&tool),&t),t)},
                    "tool_result" if typ=="user"=>{call=block["tool_use_id"].clone();("tool_output",visible(&block["content"]))},_=>continue};
                let identity=format!("{}:{b}",row.get("uuid").map(Value::to_string).unwrap_or(i.to_string()));
                if seen.insert(identity){pending.push((k.into(),t,tool,call,files,format!("event-{}-{b}",i+1),block["is_error"]==true));}
            }
        }else{
            let mut tool=Value::Null;let mut call=Value::Null;let mut files=vec![];
            let (k,t)=match typ {
                "thread.started"=>{session["id"]=row["thread_id"].clone();session["format"]=json!("codex-exec-json");continue;},
                "response_item"=>match s(&p["type"]){
                    "message" if ["user","assistant"].contains(&s(&p["role"]))&&p["channel"]!="analysis"&&p["phase"]!="analysis"=>(s(&p["role"]),visible(&p["content"])),
                    "function_call"|"custom_tool_call"=>{tool=p["name"].clone();call=p["call_id"].clone();let t=visible(p.get("arguments").or_else(||p.get("input")).unwrap_or(&Value::Null));files=regex::Regex::new(r"\*\*\* (?:Add|Update|Delete) File: (.+)").unwrap().captures_iter(&t).map(|c|c[1].to_owned()).collect();(kind(s(&tool),&t),t)},
                    "function_call_output"|"custom_tool_call_output"=>{call=p["call_id"].clone();("tool_output",visible(&p["output"]))},_=>continue},
                "event_msg" if ["user_message","agent_message"].contains(&s(&p["type"]))=>(if p["type"]=="user_message"{"user"}else{"assistant"},visible(&p["message"])),
                "item.completed"=>{let item=&row["item"];match s(&item["type"]){
                    "agent_message"=>("assistant",s(&item["text"]).into()),
                    "command_execution"=>{tool=json!("shell");(kind("shell",s(&item["command"])),format!("{}\n{}",s(&item["command"]),s(&item["aggregated_output"])))},
                    "file_change"=>{tool=json!("file_change");files=arr(&item["changes"]).iter().filter_map(|c|c["path"].as_str().map(str::to_owned)).collect();("change",item["changes"].to_string())},
                    "mcp_tool_call"=>{tool=item["tool"].clone();("tool_call",format!("{}\n{}",visible(&item["arguments"]),visible(&item["result"])))},_=>continue}},_=>continue};
            let key=if call.is_string(){format!("{k}:{}",s(&call))}else if tool.is_string(){format!("{k}:line:{i}")}else{format!("{k}:{}",short(&redact(&t),16000))};if seen.insert(key){pending.push((k.into(),t,tool,call,files,format!("event-{}",i+1),p["is_error"]==true||row["item"]["status"]=="failed"));}
        }
        for (kind,text,tool,call_id,mut files,id,failed) in pending{if !text.is_empty(){
            let code_edits=edits::extract(s(&tool),&text);
            if !code_edits.is_empty(){files.clear();}
            for edit in &code_edits{let file=s(&edit["file"]).to_owned();if !files.contains(&file){files.push(file);}}
            events.push(json!({"id":id,"kind":if code_edits.is_empty(){kind.as_str()}else{"change"},"text":short(&redact(&text),16000),"source_line":i+1,"tool":tool,"call_id":call_id,"files":files,"timestamp":row["timestamp"],"failed":failed,"code_edits":code_edits}));
        }}
    }
    session["events"]=json!(events);session["warnings"]=json!(warnings);crate::validate("Session",&session)?;Ok(session)
}
pub fn saved(review:&Value)->Result<Vec<Value>>{
    let root=Path::new(s(&review["root"]));let store=Store::open(root)?;
    let mut keys:Vec<_>=arr(&review["sessions"]).iter().map(|r|s(&r["storage_key"]).to_owned()).collect();
    if keys.is_empty()&&review["session_id"].is_string(){keys.push(s(&review["session_id"]).into());}
    let mut result=vec![];for key in keys{
        let session=match store.get("session",&key){Ok(v)=>v,Err(e) if e.to_string().starts_with("No session named")=>continue,Err(e)=>return Err(e)};
        crate::validate("Session",&session)?;
        if !belongs(s(&session["cwd"]),root){bail!("Cached session is not verifiably scoped to this repository; create a fresh review");}
        result.push(session);
    }Ok(result)
}
