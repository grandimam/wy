use anyhow::{Result,bail};
use serde_json::{Value,json};
use std::path::Path;
use crate::{arr,s,n,security::{read_source,redact,digest},history};
pub fn select<'a>(decision:&'a Value,target:&str)->Result<&'a Value>{
    let evidence=arr(&decision["evidence"]);if let Ok(i)=target.parse::<usize>(){if let Some(e)=evidence.get(i.wrapping_sub(1)){return Ok(e);}}
    if let Some(e)=evidence.iter().find(|e|e["id"]==target){return Ok(e);}bail!("Use an evidence number or citation ID from this decision")
}
pub fn inspect(review:&Value,decision:&Value,evidence:&Value)->Result<Value>{
    let mut result=json!({"review_id":review["id"],"root":review["root"],"decision_id":decision["id"],"session_id":evidence.get("session_id").filter(|v|!v.is_null()).unwrap_or(&review["session_id"]),"agent":evidence["agent"],"evidence":evidence,"related_decisions":[],"file_decisions":[]});
    for(i,d)in arr(&review["decisions"]).iter().enumerate(){
        if d["id"]!=decision["id"]&&arr(&d["evidence"]).iter().any(|e|e["id"]==evidence["id"]){result["related_decisions"].as_array_mut().unwrap().push(json!({"number":i+1,"id":d["id"],"question":d["question"]}));}
        if d["location"]["file"]==evidence["file"]{result["file_decisions"].as_array_mut().unwrap().push(json!({"number":i+1,"id":d["id"],"question":d["question"],"line":d["location"]["start_line"],"stale":d["stale"]}));}
    }
    if evidence["kind"]=="session"{
        result["context"]=json!([]);result["linked_tool_events"]=json!([]);
        for session in history::saved(review)?{
            if session["path"]!=evidence["file"]||!evidence["session_id"].is_null()&&session["id"]!=evidence["session_id"]||!evidence["agent"].is_null()&&session["agent"]!=evidence["agent"]{continue;}
            result["agent"]=session["agent"].clone();result["session_id"]=session["id"].clone();let events=arr(&session["events"]);
            if let Some(i)=events.iter().position(|e|e["id"]==evidence["event_id"]){result["context"]=json!(&events[i.saturating_sub(2)..(i+3).min(events.len())]);if !events[i]["call_id"].is_null(){result["linked_tool_events"]=json!(events.iter().filter(|e|e["call_id"]==events[i]["call_id"]&&e["id"]!=events[i]["id"]).collect::<Vec<_>>());}}
            break;
        }
        result["note"]=json!("Nearby events are chronological context, not proof of cause or authorship.");return Ok(result);
    }
    result["current"]=Value::Null;
    let Some(raw)=read_source(Path::new(s(&review["root"])),s(&evidence["file"]))else{result["note"]=json!("Current source is missing or unavailable; the saved excerpt remains inspectable.");return Ok(result)};
    let text=redact(&raw);let current:Vec<_>=text.lines().collect();let excerpt:Vec<_>=s(&evidence["excerpt"]).lines().collect();let unchanged=digest(&raw)==s(&evidence["snapshot_hash"]);
    let matches:Vec<_>=if excerpt.is_empty(){vec![]}else{current.windows(excerpt.len()).enumerate().filter(|(_,w)|*w==excerpt.as_slice()).map(|(i,_)|i+1).collect()};
    let(line,status)=if unchanged{(n(&evidence["start_line"]),"unchanged")}else if matches.len()==1{(matches[0],"unique_excerpt_match")}else{(n(&evidence["start_line"]).min(current.len().max(1)),if matches.len()>1{"ambiguous"}else{"excerpt_changed"})};
    let start=line.saturating_sub(6).max(1);let end=(line+excerpt.len().max(1)+5).min(current.len());
    result["current"]=json!({"status":status,"file_unchanged":unchanged,"matching_lines":matches,"start_line":start,"end_line":end,"excerpt":current.iter().skip(start-1).take(end.saturating_sub(start)+1).copied().collect::<Vec<_>>().join("\n"),"anchor_line":line});
    result["note"]=json!(if unchanged{"Current file matches the review snapshot."}else{"File changed since review. Matching text is a navigation aid, not revalidated evidence; the original citation is preserved."});Ok(result)
}
