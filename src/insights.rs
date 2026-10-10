//! Offline decision context and review briefs. No causal claims are inferred here.
use crate::{arr, s, history, security};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path, io::Write};

pub const DECISION_INSTRUCTIONS: &str = include_str!("data/decision-instructions.md");

/// Always show an absolute UTC date as well as age. Unknown timestamps are not
/// replaced with file mtimes, which can change when transcripts are copied.
pub fn when(value:&Value)->String{
    let Some(date)=value.as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok()) else{return "date unknown".into();};
    let date=date.with_timezone(&chrono::Utc);
    let age=chrono::Utc::now().signed_duration_since(date);
    let relative=if age.num_minutes()<0{"future timestamp; check source clock".into()}else if age.num_days()>0{format!("{}d ago",age.num_days())}else if age.num_hours()>0{format!("{}h ago",age.num_hours())}else{format!("{}m ago",age.num_minutes())};
    format!("{} UTC · {relative}",date.format("%Y-%m-%d %H:%M"))
}
/// Age of the capture, distinct from the source event's date.
pub fn capture_age(value:&Value)->String {
    let Some(date)=value.as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok()) else{return "at an unknown time".into();};
    let age=chrono::Utc::now().signed_duration_since(date);
    if age.num_seconds()<0 {"at a future timestamp (check clock)".into()}
    else if age.num_days()>0 {format!("{} days ago",age.num_days())}
    else if age.num_hours()>0 {format!("{} hours ago",age.num_hours())}
    else if age.num_minutes()>0 {format!("{} min ago",age.num_minutes())}
    else {"just now".into()}
}
/// Local wall-clock date for reading; `when` keeps the UTC form for details and exports.
pub fn local_date(value:&Value)->String{
    let Some(date)=value.as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok()) else{return "date unknown".into();};
    let local=date.with_timezone(&chrono::Local);let today=chrono::Local::now().date_naive();
    let day=if local.date_naive()==today{"Today".to_owned()}else if today.pred_opt()==Some(local.date_naive()){"Yesterday".to_owned()}else{local.format("%b %-d, %Y").to_string()};
    format!("{day} {}",local.format("%-I:%M %p"))
}
pub fn session_dates(session:&Value)->(Value,Value){
    let mut dates:Vec<_>=arr(&session["events"]).iter().filter_map(|e|e["timestamp"].as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d|(d,e["timestamp"].clone()))).collect();
    dates.sort_by_key(|(d,_)|*d);
    let start=session.get("started_at").filter(|v|v.is_string()).cloned().unwrap_or_else(||dates.first().map(|(_,d)|d.clone()).unwrap_or(Value::Null));
    let last=dates.last().map(|(_,d)|d.clone()).unwrap_or(start.clone());(start,last)
}

/// Only an original assistant statement can declare a decision record. Treat its
/// contents as self-reported assertions, not validated facts or executable inputs.
pub fn decision(event:&Value)->Option<Value>{
    if event["kind"]!="assistant" || !history::provenance::original(event){return None;}
    let text=s(&event["text"]);let (_,body)=text.split_once("WY_DECISION")?;
    let body=body.trim_start().strip_prefix("```json").unwrap_or(body.trim_start()).trim_start();
    let record=serde_json::Deserializer::from_str(body).into_iter::<Value>().next()?.ok()?;
    let file=record["file"].as_str()?;
    if !security::allowed(file)||s(&record["decision"]).is_empty()||s(&record["reason"]).is_empty(){return None;}
    let mut result=json!({"file":file});
    for field in ["symbol","decision","reason","requirement","timing","validation"]{
        if let Some(v)=record[field].as_str(){result[field]=json!(v);}
    }
    for field in ["alternatives","tradeoffs","evidence","related_edits"]{
        result[field]=json!(arr(&record[field]).iter().filter(|v|v.is_string()).take(32).cloned().collect::<Vec<_>>());
    }
    Some(result)
}

pub fn evidence_label(event:&Value)->String{
    if decision(event).is_some(){return "Recorded decision record · self-reported, verify evidence".into();}
    match s(&event["kind"]){
        "assistant" if history::provenance::original(event)=>"Recorded statement · relevance is not proof of causation".into(),
        "user" if history::provenance::original(event)=>"Recorded user request".into(),
        _=>history::provenance::label(event)
    }
}
fn mentions(text:&str,term:&str)->bool{
    if term.is_empty(){return false;}
    regex::Regex::new(&format!(r"(?:^|[^\w./-]){}(?:$|[^\w./-])",regex::escape(term))).unwrap().is_match(text)
}
/// Full user-bounded turns around edits, not a merged conversation across agents.
/// A symbol filter is an explicit text/record match, not semantic attribution.
pub fn timeline(root:&Path,sessions:&[Value],file:&str,symbol:Option<&str>)->Vec<Value>{
    let edits=history::attribution::edits(root,sessions,Some(file));
    let mut result=vec![];
    for session in sessions {
        let events=arr(&session["events"]);let mut starts=HashSet::new();
        let relevant:Vec<_>=edits.iter().filter(|e|e["session_id"]==session["id"]&&e["agent"]==session["agent"]).collect();
        for (at,event) in events.iter().enumerate(){
            let edit=relevant.iter().any(|e|e["event_id"]==event["id"]);
            let explicit=decision(event).is_some_and(|r|r["file"]==file)||mentions(s(&event["text"]),file);
            if !edit&&!explicit{continue;}
            let start=events[..=at].iter().rposition(|e|e["kind"]=="user").unwrap_or(0);
            if !starts.insert(start){continue;}
            let end=events[start+1..].iter().position(|e|e["kind"]=="user").map(|i|start+1+i).unwrap_or(events.len());
            let turn=&events[start..end];
            if symbol.filter(|s|!s.is_empty()).is_some_and(|symbol|!turn.iter().any(|e|mentions(s(&e["text"]),symbol)||decision(e).is_some_and(|r|r["symbol"]==symbol))){continue;}
            let related:Vec<_>=relevant.iter().filter(|e|turn.iter().any(|event|event["id"]==e["event_id"])).map(|e|(*e).clone()).collect();
            let records:Vec<_>=turn.iter().filter_map(|e|decision(e).map(|r|json!({"record":r,"event_id":e["id"]}))).filter(|r|r["record"]["file"]==file).collect();
            let outcomes:Vec<_>=turn.iter().filter(|e|e["kind"]=="test").map(|test|{
                let output=turn.iter().find(|e|e["kind"]=="tool_output"&&test["call_id"].is_string()&&e["call_id"]==test["call_id"]);
                json!({"event_id":test["id"],"command":test["text"],"outcome":if test["failed"]==true||output.is_some_and(|e|e["failed"]==true){"tool reported failure"}else if output.is_some(){"output recorded; inspect assertions and exit status"}else{"completion/result not captured"},"output":output})
            }).collect();
            result.push(json!({"agent":session["agent"],"session_id":session["id"],"session_path":session["path"],
                "timestamp":events[start]["timestamp"],"anchor":events[start]["id"],"match":if related.is_empty(){"explicit file mention"}else{"recorded edit to file"},
                "events":turn,"edits":related,"records":records,"validation":outcomes}));
        }
    }
    result.sort_by(|a,b|s(&a["timestamp"]).cmp(s(&b["timestamp"])).then_with(||s(&a["agent"]).cmp(s(&b["agent"]))).then_with(||s(&a["session_id"]).cmp(s(&b["session_id"]))));
    result
}

/// Stable, bounded, offline brief. Excludes raw tool output and rationale by default.
/// Redaction is best-effort: the user must review the exact preview before saving.
pub fn brief(review:&Value,sessions:&[Value])->String{
    let root=Path::new(s(&review["root"]));
    let mut text=format!("# wy review brief\n\nCapture: {}\nGit base: {}\n\nThis is captured evidence, not approval or proof of intent. Statements are self-reported.\nRedaction is best-effort: review for sensitive information before sharing.\nRaw tool outputs, full transcripts and tentative rationale are excluded.\n\n",s(&review["created_at"]),s(&review["head"]));
    text.push_str("## History coverage\n");
    for row in arr(&review["coverage"]){text.push_str(&format!("- {}: enabled={}, discovered={}, captured={}, budget exclusions={}, unreadable={}, scope/identity exclusions={}\n",s(&row["agent"]),row["enabled"],row["discovered"],row["captured"],row["skipped_budget"],row["skipped_unreadable"],row["skipped_scope"]));}
    if arr(&review["coverage"]).is_empty(){text.push_str("Coverage diagnostics unavailable for this older capture.\n");}
    let files:Vec<_>=arr(&review["changes"]).iter().filter_map(|c|c["file"].as_str()).collect();
    if files.is_empty(){text.push_str("\nNo working-tree changes in this capture. Use /timeline FILE for historical context.\n");}
    for file in files.iter().take(40){
        text.push_str(&format!("\n## {file}\n"));
        let changes=history::attribution::reasons(root,sessions,file,arr(&review["changes"]).iter().find(|c|c["file"]==*file).and_then(|c|c["diff"].as_str()));
        let unmatched=arr(&changes["hunks"]).iter().filter(|h|h["status"]=="none").count();
        text.push_str(&format!("{} diff hunks; {unmatched} without matching edit records. Text overlap does not prove authorship.\n",arr(&changes["hunks"]).len()));
        let contexts=timeline(root,sessions,file,None);
        let records:Vec<_>=contexts.iter().flat_map(|c|arr(&c["records"]).iter().map(move|r|(c,r))).collect();
        if records.is_empty(){text.push_str("- No structured decision record captured. Original intent may be unknown; use /decisions to inspect nearby statements.\n");}
        for (context,r) in records.iter().rev().take(8){let record=&r["record"];
            text.push_str(&format!("- Recorded decision (self-reported): {}\n  Reason: {}\n  Requirement: {}\n  Symbol: {}\n  Timing: {}\n  Source: {} session {}, event {}\n",security::short(s(&record["decision"]),500),security::short(s(&record["reason"]),800),security::short(s(&record["requirement"]),400),s(&record["symbol"]),s(&record["timing"]),s(&context["agent"]),s(&context["session_id"]),s(&r["event_id"])));
        }
        if records.len()>8{text.push_str("- Additional decision records omitted from this bounded brief.\n");}
        let tests:Vec<_>=contexts.iter().flat_map(|c|arr(&c["validation"]).iter().map(move |t|(c,t))).collect();
        if tests.is_empty(){text.push_str("- Validation gap: no test command captured in the related turns. This does not prove tests were not run.\n");}
        for (c,t) in tests.iter().rev().take(4){text.push_str(&format!("- Validation: {}. Source: {} session {}, event {}\n",s(&t["outcome"]),s(&c["agent"]),s(&c["session_id"]),s(&t["event_id"])));}
        text.push_str("- Reviewer checks: confirm requirements, assumptions, failure cases, and test coverage. Later edits may make earlier explanations inapplicable.\n");
        if text.len()>160_000{text.push_str("\nBrief size limit reached; additional files omitted.\n");break;}
    }
    if files.len()>40{text.push_str("\nOnly the first 40 changed files are included.\n");}
    text.push_str("\n## Capture warnings\n");for warning in arr(&review["warnings"]).iter().take(20){text.push_str(&format!("- {}\n",security::short(s(warning),600)));}
    let mut text=security::redact(&text);
    if let Some(root)=review["root"].as_str().filter(|s|!s.is_empty()){text=text.replace(root,"<repo>");}
    if let Some(home)=std::env::var_os("HOME").filter(|s|!s.is_empty()){text=text.replace(home.to_string_lossy().as_ref(),"<home>");}
    text
}

/// Write only beneath wy's private store; never overwrite an existing artifact or
/// follow a symlink. Both exports and optional instructions require an explicit UI action.
pub fn save_private(root:&Path,name:&str,text:&str)->Result<std::path::PathBuf>{
    ensure!(!name.is_empty()&&name.chars().all(|c|c.is_ascii_alphanumeric()||"-_.".contains(c))&&name!="."&&name!="..","Invalid artifact name");
    crate::storage::Store::open(root)?;
    let path=root.join(".wy").join(name);
    let mut options=std::fs::OpenOptions::new();options.write(true).create_new(true);
    #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;options.mode(0o600).custom_flags(libc::O_NOFOLLOW);}
    let mut file=options.open(&path)?;file.write_all(text.as_bytes())?;file.sync_all()?;Ok(path)
}
