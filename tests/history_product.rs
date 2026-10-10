use serde_json::{Value,json};
use std::{fs,path::Path,process::Command};
use wy::{arr,s,history,insights,service};

fn repo()->tempfile::TempDir{
    let dir=tempfile::tempdir().unwrap();assert!(Command::new("git").args(["init","-q"]).arg(dir.path()).status().unwrap().success());dir
}
fn jsonl(path:&Path,rows:&[Value]){fs::write(path,rows.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")).unwrap();}
fn pi(path:&Path,root:&Path)->Value{
    jsonl(path,&[
        json!({"type":"session","id":"same-id","cwd":root,"timestamp":"2024-01-01T00:00:00Z"}),
        json!({"type":"message","id":"u","parentId":null,"timestamp":"2024-01-01T00:01:00Z","message":{"role":"user","content":"Add src/cache.rs :: Cache"}}),
        json!({"type":"message","id":"abandoned","parentId":"u","message":{"role":"assistant","content":[{"type":"text","text":"ABANDONED BRANCH"}]}}),
        json!({"type":"message","id":"a","parentId":"u","timestamp":"2024-01-01T00:02:00Z","message":{"role":"assistant","model":"test-model","content":[{"type":"thinking","thinking":"Consider a Cache in src/cache.rs.","thinkingSignature":"ENCRYPTED_SECRET"},{"type":"toolCall","id":"write1","name":"write","arguments":{"path":"src/cache.rs","content":"struct Cache;"}}]}}),
        json!({"type":"message","id":"r","parentId":"a","timestamp":"2024-01-01T00:03:00Z","message":{"role":"toolResult","toolCallId":"write1","content":[{"type":"text","text":"Successfully wrote file"}],"isError":false}}),
        json!({"type":"compaction","id":"c","parentId":"r","timestamp":"2024-01-01T00:04:00Z","summary":"Added src/cache.rs"}),
    ]);
    history::collect(path).unwrap()
}
#[test]
fn pi_captures_active_branch_rationale_edits_dates_and_summaries(){
    let dir=repo();let root=dir.path().canonicalize().unwrap();let session=pi(&root.join("pi.jsonl"),&root);
    assert_eq!(session["agent"],"pi");assert_eq!(session["started_at"],"2024-01-01T00:00:00Z");
    assert!(!session.to_string().contains("ABANDONED BRANCH"));assert!(!session.to_string().contains("ENCRYPTED_SECRET"));
    let rationale=arr(&session["events"]).iter().find(|e|e["kind"]=="rationale").unwrap();
    assert!(!history::provenance::original(rationale));assert!(history::provenance::label(rationale).contains("tentative"));
    let mut evidence=history::event_evidence(&session,rationale);
    evidence["origin_status"]=json!("unavailable");evidence["origin_reason"]=json!("provenance was not classified; re-import the transcript");
    history::origins::enrich(&root,&mut evidence).unwrap();
    assert!(history::origins::status(&evidence).is_none());assert!(evidence["origin_status"].is_null());
    assert_eq!(rationale["source_line"],4);
    assert!(arr(&session["events"]).iter().any(|e|e["kind"]=="summary"));
    let edits=history::attribution::edits(&root,&[session.clone()],Some("src/cache.rs"));assert_eq!(edits.len(),1);
    assert_eq!(edits[0]["text"],"struct Cache;");
    assert_eq!(insights::session_dates(&session).1,"2024-01-01T00:04:00Z");
    assert!(insights::when(&session["started_at"]).contains("2024-01-01 00:00 UTC"));
    let contexts=insights::timeline(&root,&[session],"src/cache.rs",Some("Cache"));assert_eq!(contexts.len(),1);
    assert_eq!(arr(&contexts[0]["edits"]).len(),1);
}
#[test]
fn pi_malformed_lines_keep_source_positions_and_cycles_are_rejected(){
    let dir=repo();let root=dir.path().canonicalize().unwrap();let path=root.join("pi.jsonl");
    let header=json!({"type":"session","id":"p","cwd":root});
    fs::write(&path,format!("{header}\nmalformed\n{}",json!({"type":"message","id":"a","message":{"role":"user","content":"Hello"}}))).unwrap();
    let session=history::collect(&path).unwrap();assert_eq!(session["events"][0]["source_line"],3);
    jsonl(&path,&[header,json!({"type":"message","id":"a","parentId":"a","message":{"role":"user","content":"cycle"}})]);
    assert!(history::collect(&path).unwrap_err().to_string().contains("Cycle"));
}
fn opencode(path:&Path,root:&Path,failed:bool)->Value{
    let db=rusqlite::Connection::open(path).unwrap();
    db.execute_batch("CREATE TABLE session(id TEXT,directory TEXT,time_created INTEGER); CREATE TABLE message(id TEXT,session_id TEXT,time_created INTEGER,data TEXT); CREATE TABLE part(id TEXT,message_id TEXT,session_id TEXT,time_created INTEGER,data TEXT);").unwrap();
    db.execute("INSERT INTO session VALUES ('same-id',?,1704153600000)",[root.to_string_lossy().as_ref()]).unwrap();
    db.execute("INSERT INTO message VALUES ('m','same-id',1704153600000,?)",[json!({"role":"assistant","modelID":"model-x"}).to_string()]).unwrap();
    for (id,data) in [
        ("a",json!({"type":"reasoning","text":"A Cache can isolate storage.","metadata":{"encrypted_content":"HIDDEN"}})),
        ("b",json!({"type":"tool","tool":"edit","callID":"edit1","state":{"status":if failed{"error"}else{"completed"},"input":{"filePath":"src/cache.rs","oldString":"struct Cache;","newString":"struct Cache { ttl: u64 }"},"output":"Changed file","error":"failed"}})),
        ("c",json!({"type":"text","text":"Changed src/cache.rs :: Cache to store a TTL."})),
    ]{db.execute("INSERT INTO part VALUES (?,'m','same-id',1704153600000,?)",rusqlite::params![id,data.to_string()]).unwrap();}
    drop(db);
    history::collect_entry(&json!({"agent":"opencode","path":path,"id":"same-id"})).unwrap()
}
#[test]
fn opencode_is_read_only_preserves_tool_results_and_handoffs(){
    let dir=repo();let root=dir.path().canonicalize().unwrap();let path=root.join("opencode.db");
    let session=opencode(&path,&root,false);let before=fs::read(&path).unwrap();
    let again=history::collect_entry(&json!({"agent":"opencode","path":path,"id":"same-id"})).unwrap();
    assert_eq!(before,fs::read(&path).unwrap());assert_eq!(session,again);
    assert_eq!(session["agent"],"opencode");assert!(!session.to_string().contains("HIDDEN"));
    assert_eq!(arr(&session["events"]).iter().filter(|e|e["kind"]=="rationale").count(),1);
    assert!(arr(&session["events"]).iter().any(|e|e["kind"]=="tool_output"&&e["call_id"]=="edit1"));
    let prior=pi(&root.join("pi.jsonl"),&root);
    let contexts=insights::timeline(&root,&[session,prior],"src/cache.rs",None);
    assert_eq!(contexts.len(),2);assert_eq!(contexts[0]["agent"],"pi");assert_eq!(contexts[1]["agent"],"opencode");
    assert_ne!(contexts[0]["timestamp"],contexts[1]["timestamp"]);
}
#[test]
fn failed_opencode_edits_are_not_attributed_as_written_code(){
    let dir=repo();let root=dir.path().canonicalize().unwrap();let session=opencode(&root.join("db"),&root,true);
    assert!(history::attribution::edits(&root,&[session],None).is_empty());
}
fn record()->Value{
    json!({"id":"decision","kind":"assistant","timestamp":"2024-01-01T00:02:00Z","provenance":{"source_type":"original_turn"},"text":format!("WY_DECISION\n{}",json!({"file":"src/cache.rs","symbol":"Cache","decision":"Cache results","reason":"Avoid repeated reads","requirement":"Faster reads","timing":"decision-time","alternatives":["No cache"],"validation":"not run"}))})
}
#[test]
fn structured_records_require_original_assistant_speech_and_safe_files(){
    let original=record();assert_eq!(insights::decision(&original).unwrap()["symbol"],"Cache");
    for kind in ["user","tool_output","rationale","summary"]{let mut event=original.clone();event["kind"]=json!(kind);assert!(insights::decision(&event).is_none());}
    let mut event=original.clone();event["provenance"]["source_type"]=json!("compaction_summary");assert!(insights::decision(&event).is_none());
    event=original;event["text"]=json!(s(&event["text"]).replace("src/cache.rs","../private.rs"));assert!(insights::decision(&event).is_none());
}
#[test]
fn brief_is_bounded_redacted_and_does_not_include_raw_rationale_or_tool_output(){
    let dir=repo();let root=dir.path().canonicalize().unwrap();
    let session=json!({"id":"s","agent":"pi","events":[record(),{"id":"private","kind":"rationale","text":"NEVER_EXPORT_RATIONALE"},{"id":"tool","kind":"tool_output","text":"NEVER_EXPORT_OUTPUT"}]});
    let review=json!({"root":root,"created_at":"2024-01-01T00:00:00Z","changes":[{"file":"src/cache.rs","diff":"+struct Cache;"}],"warnings":["api_key=sk-123456789abcdefghijk"]});
    let brief=insights::brief(&review,&[session]);assert!(brief.contains("Avoid repeated reads"));
    assert!(!brief.contains("NEVER_EXPORT"));assert!(!brief.contains("sk-123456789"));assert!(brief.contains("Redaction is best-effort"));
    let saved=insights::save_private(&root,"brief.md",&brief).unwrap();assert_eq!(fs::read_to_string(saved).unwrap(),brief);
    assert!(insights::save_private(&root,"brief.md","overwrite").is_err());assert!(insights::save_private(&root,"../escape.md","bad").is_err());
    #[cfg(unix)]{std::os::unix::fs::symlink(root.join("outside"),root.join(".wy/link.md")).unwrap();assert!(insights::save_private(&root,"link.md","bad").is_err());assert!(!root.join("outside").exists());}
}
#[test]
fn coverage_and_source_filters_are_explicit_and_backward_compatible(){
    let dir=repo();let root=dir.path().canonicalize().unwrap();
    let review=service::review(&root,&service::ReviewOptions{source:"none".into()}).unwrap();
    assert_eq!(arr(&review["coverage"]).len(),4);assert!(arr(&review["coverage"]).iter().all(|r|r["enabled"]==false));
    assert!(history::source_matches("both","pi"));assert!(history::source_matches("all","opencode"));assert!(!history::source_matches("pi","claude"));
    let local=root.join(".pi/sessions");fs::create_dir_all(&local).unwrap();pi(&local.join("test.jsonl"),&root);
    fs::write(local.join("bad.jsonl"),"invalid").unwrap();
    let review=service::review(&root,&service::ReviewOptions{source:"pi".into()}).unwrap();
    let status=arr(&review["coverage"]).iter().find(|r|r["agent"]=="pi").unwrap();
    assert_eq!(status["discovered"],1);assert_eq!(status["captured"],1);assert!(!arr(&status["issues"]).is_empty());
    let mut old=review;old.as_object_mut().unwrap().remove("coverage");wy::validate("Review",&old).unwrap();
}
#[test]
fn unknown_dates_do_not_use_file_modification_time(){
    assert_eq!(insights::when(&Value::Null),"date unknown");
    assert_eq!(insights::session_dates(&json!({"events":[]})),(Value::Null,Value::Null));
    assert!(insights::when(&json!("2999-01-01T00:00:00Z")).contains("future timestamp"));
    let (start,last)=insights::session_dates(&json!({"events":[{"timestamp":"2024-01-01T00:00:00Z"},{"timestamp":"2024-01-01T02:00:00+04:00"}]}));
    assert_eq!(start,"2024-01-01T02:00:00+04:00");assert_eq!(last,"2024-01-01T00:00:00Z");
}
