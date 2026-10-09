use serde_json::{Value,json};
use crate::{s,n,arr};
#[derive(Clone,Debug,PartialEq)]
pub enum Tone{Normal,Heading,Muted,Warning,Choice}
#[derive(Clone,Debug)]
pub struct Paragraph{pub text:String,pub tone:Tone}
fn judgments(result:&Value)->Vec<Value>{arr(&result["judgments"]).iter().map(|j|json!({"text":format!("{}\n{}{}",s(&j["choice"]),s(&j["reason"]),if s(&j["quote"]).is_empty(){String::new()}else{format!("\nRecorded statement: “{}”",s(&j["quote"]))}),"basis":j["status"],"evidence_ids":j["evidence_ids"]})).collect()}
/// Stable numbered references, including focused answers.
pub fn citations(artifact:&Value)->Vec<Value>{
    let result=&artifact["explanation"];let mut claims=judgments(result);
    if artifact["packet"]["focus_target"].is_null(){claims.push(result["answer"].clone());}
    claims.push(result["problem"].clone());
    if artifact["packet"]["focus_target"].is_null(){claims.extend([result["before"].clone(),result["after"].clone()]);}
    for key in ["steps","tradeoffs","checks"]{claims.extend(arr(&result[key]).iter().cloned());}
    claims.extend([result["answer"].clone(),result["before"].clone(),result["after"].clone()]);
    let mut cited=vec![];
    for claim in claims{for id in arr(&claim["evidence_ids"]){if !cited.iter().any(|e:&Value|e["id"]==*id){if let Some(e)=arr(&artifact["packet"]["evidence"]).iter().find(|e|e["id"]==*id){cited.push(e.clone());}}}}cited
}
pub fn reasoning(artifact:&Value)->Vec<Paragraph>{
    let mut checked=artifact.clone();crate::history::provenance::sanitize_artifact(&mut checked);let artifact=&checked;
    let result=&artifact["explanation"];let cited=citations(artifact);let mut out=vec![Paragraph{text:s(&result["title"]).into(),tone:Tone::Heading},Paragraph{text:"Based on captured evidence".into(),tone:Tone::Muted}];
    if let Some(warning)=artifact["provenance_warning"].as_str(){out.push(Paragraph{text:warning.into(),tone:Tone::Warning});}
    if !arr(&artifact["stale_files"]).is_empty()||artifact["stale_head"]==true{out.push(Paragraph{text:"EARLIER SNAPSHOT · files or Git HEAD changed during generation. Explain again for the latest state.".into(),tone:Tone::Warning});}
    let js=judgments(result);let mut reasons=if js.is_empty(){vec![result["problem"].clone()]}else{js};reasons.extend(arr(&result["steps"]).iter().cloned());reasons.extend(arr(&result["tradeoffs"]).iter().cloned());
    for(title,claims)in [("The change",vec![result["answer"].clone()]),("The reasoning",reasons),("What to check",arr(&result["checks"]).to_vec())]{
        if claims.is_empty(){continue;}out.push(Paragraph{text:title.into(),tone:Tone::Heading});
        for claim in claims{
            let refs=arr(&claim["evidence_ids"]).iter().filter_map(|id|cited.iter().position(|e|e["id"]==*id)).map(|i|format!(" [{}]",i+1)).collect::<String>();
            let label=match s(&claim["basis"]){"observed"=>"Evidence","assessment"=>"Interpretation","proposed"=>"Suggestion","unknown"=>"Not known","recorded"=>"Stated reason","inferred"=>"Inferred reason",_=>""};
            out.push(Paragraph{text:format!("{}{refs}  ({label})",s(&claim["text"])),tone:if ["recorded","inferred","unknown"].contains(&s(&claim["basis"])){Tone::Choice}else{Tone::Normal}});
        }
    }
    if !arr(&result["unknowns"]).is_empty(){out.push(Paragraph{text:"Still unknown".into(),tone:Tone::Warning});for item in arr(&result["unknowns"]){let mut text=s(item).to_owned();for e in arr(&artifact["packet"]["evidence"]){text=text.replace(s(&e["id"]),s(&e["file"]));}out.push(Paragraph{text,tone:Tone::Normal});}}
    out
}
pub fn reasoning_text(artifact:&Value)->String{let mut out=reasoning(artifact).iter().map(|p|p.text.as_str()).collect::<Vec<_>>().join("\n\n");out.push_str(&format!("\n\nSaved: {}\nInspect: wy reasoning-evidence 1 --id {}\n",s(&artifact["id"]),s(&artifact["id"])));out}
pub fn decision(d:&Value,show_code:bool)->String{
    let mut checked=d.clone();crate::history::provenance::sanitize_decision(&mut checked);let d=&checked;
    let loc=&d["location"];let mut out=format!("{} · {}{}\n{}:{} · {} · {}\n\n{}\n",s(&d["question"]),s(&d["provenance"]),if d["stale"]==true{" · STALE — re-review"}else{""},s(&loc["file"]),n(&loc["start_line"]),s(&loc["symbol"]),s(&d["id"]),s(&d["explanation"]));
    for(i,e)in arr(&d["evidence"]).iter().enumerate(){out.push_str(&format!("\n[{}] {}:{} · {}\n",i+1,s(&e["file"]),n(&e["start_line"]),s(&e["id"])));if e["kind"]=="session"{out.push_str(&crate::history::provenance::label(e));out.push('\n');if let Some(status)=crate::history::origins::status(e){out.push_str(&status);out.push('\n');}}if show_code{out.push_str(s(&e["excerpt"]));out.push('\n');}}
    for(label,key)in [("Alternative to investigate","alternatives"),("Assumption","assumptions"),("Open question","unresolved_questions")]{for item in arr(&d[key]){out.push_str(&format!("\n{label}: {}",s(item)));}}
    for r in arr(&d["reflections"]){out.push_str(&format!("\n\nRetrospective assessment: {} · {} · identity self-reported\n{}\nUncertainty: {}\nSuggested change: {}",s(&r["assessment"]),s(&r["agent"]),s(&r["rationale"]),s(&r["uncertainty"]),s(&r["suggested_change"])));}
    out
}
pub fn render(value:&Value)->String{
    if value["commit"].is_string(){return commit_context(value);}
    if value["explanation"].is_object()&&value["packet"].is_object(){return reasoning_text(value);}
    if value["question"].is_string()&&value["location"].is_object(){return decision(value,false);}
    if value["decisions"].is_array(){let mut out=format!("{} · {} changed files · {} decisions\n",s(&value["root"]),arr(&value["changes"]).len(),arr(&value["decisions"]).len());for w in arr(&value["warnings"]){out.push_str(&format!("Note: {}\n",s(w)));}for(i,d)in arr(&value["decisions"]).iter().enumerate(){out.push_str(&format!("\n{}. {}\n",i+1,decision(d,false)));}return out;}
    if value["answer"].is_string(){return format!("{}\n\n{}\nEvidence: {}",s(&value["answer"]),s(&value["uncertainty"]),arr(&value["evidence_ids"]).iter().map(s).collect::<Vec<_>>().join(", "));}
    if value.is_array() && arr(value).iter().all(|e| e["agent"].is_string() && e["path"].is_string()){if arr(value).is_empty(){return "No results.".into();}return arr(value).iter().map(|e|format!("{}:{}  {}  {}",s(&e["agent"]),s(&e["id"]),s(&e["cwd"]),s(&e["path"]))).collect::<Vec<_>>().join("\n");}
    serde_json::to_string_pretty(value).unwrap_or_default()
}

fn commit_context(value: &Value) -> String {
    let commit = s(&value["commit"]);
    if value["review_id"].is_string() {
        return format!("Linked {} to commit {commit} · {} saved conversations\nInspect: wy session --commit {commit}", s(&value["review_id"]), n(&value["session_count"]));
    }
    let mut out = format!("Commit {commit}\n");
    for link in arr(&value["links"]) {
        let basis = if link["association"] == "snapshot-match" { "matched review base and source snapshot" } else { "explicitly linked" };
        out.push_str(&format!("Review {} · {basis}\n", s(&link["review_id"])));
    }
    if arr(&value["sessions"]).is_empty() {
        out.push_str("No matching saved conversations.\n");
    }
    for session in arr(&value["sessions"]) {
        out.push_str(&format!("\n{}:{} · snapshot {}\n", s(&session["agent"]), s(&session["id"]), s(&session["storage_key"])));
        if session["events"].is_array() {
            for event in arr(&session["events"]) {
                out.push_str(&format!("\n{}\n",crate::history::provenance::label(event)));
                if let Some(status)=crate::history::origins::status(event){out.push_str(&format!("{status}\n"));}
                out.push_str(&format!("\n[{}] {} · {}:{}\n{}\n", s(&event["id"]), s(&event["kind"]), s(&session["path"]), n(&event["source_line"]), s(&event["text"])));
            }
        } else {
            out.push_str(&format!("{} saved events · {}\n", n(&session["event_count"]), s(&session["path"])));
        }
    }
    out
}
