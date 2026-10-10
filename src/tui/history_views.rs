//! Offline history navigation: preserve agent/session boundaries and visible dates.
use super::{Target, document::{Document, View, Link}, theme::*};
use crate::{arr, s, n, insights, history};
use serde_json::{Value,json};
use std::{path::Path,sync::Arc};

fn captured(doc:&mut Document,review:&Value){
    doc.text(format!("History captured: {}",insights::when(&review["created_at"])),MUTED);
    doc.text("Historical context is not necessarily current intent. r refreshes capture; source dates do not change.",AMBER);
}
pub(super) fn coverage(review:&Value,sessions:&[Value])->Document{
    let mut doc=Document::new(View::Coverage,"History coverage");captured(&mut doc,review);
    doc.text(format!("Source filter: {} · up to 20 sessions / 40 MB total / 20 MB per input",s(&review["history_source"])),MUTED);
    if arr(&review["coverage"]).is_empty(){doc.heading("Coverage unavailable for this older capture · press r");}
    for row in arr(&review["coverage"]){
        doc.heading(format!("{} · {}",s(&row["agent"]),if row["enabled"]==true{"enabled"}else{"not selected"}));
        doc.text(format!("Discovered {} · Captured {} · Budget exclusions {} · Unreadable {} · Scope/identity exclusions {} · Duplicates {}",n(&row["discovered"]),n(&row["captured"]),n(&row["skipped_budget"]),n(&row["skipped_unreadable"]),n(&row["skipped_scope"]),n(&row["duplicates"])),TEXT);
        if row["enabled"]==true && n(&row["discovered"])==0{doc.text("No matching sessions found in the supported default/configured stores. This is not proof that no agent was used.",MUTED);}
        for issue in arr(&row["issues"]){doc.text(s(issue),AMBER);}
    }
    doc.heading("Current diff linkage");
    let mut total=0;let mut missing=0;let mut partial=0;
    for change in arr(&review["changes"]){
        let edits=history::attribution::edits(Path::new(s(&review["root"])),sessions,Some(s(&change["file"])));
        let hunks=history::attribution::hunk_sources(s(&change["diff"]),&edits);
        total+=hunks.len();missing+=hunks.iter().filter(|h|h["status"]=="none").count();partial+=hunks.iter().filter(|h|h["status"]=="partial").count();
    }
    doc.text(format!("{total} hunks · {missing} without matching edit records · {partial} with partial text overlap"),TEXT);
    doc.text("Text overlap is not proof of authorship. Missing linkage may reflect shell/formatter edits, unsupported formats, excluded sessions, or later changes. It does not mean no rationale exists.",MUTED);
    doc.heading("Capture warnings");for w in arr(&review["warnings"]){doc.text(s(w),AMBER);}
    doc.heading("Next steps");doc.text("/sessions opens captured conversations. /source AGENT then r narrows capture. /timeline FILE shows handoffs. /decisions FILE:SYMBOL inspects decision context.",TEXT);doc
}
pub(super) fn sessions(review:&Value,sessions:&[Value])->Document{
    let mut doc=Document::new(View::Sessions,"Captured sessions");captured(&mut doc,review);
    let mut ordered:Vec<_>=sessions.iter().collect();ordered.sort_by_key(|session|std::cmp::Reverse(insights::session_dates(session).1.as_str().unwrap_or("").to_owned()));
    for session in ordered{
        let (start,last)=insights::session_dates(session);
        doc.heading(format!("{} · {} · open ›",s(&session["agent"]),s(&session["id"])));
        if let Some(reference)=arr(&review["sessions"]).iter().find(|r|r["agent"]==session["agent"]&&r["id"]==session["id"]){doc.sources.push((doc.lines.len()-1,Link::Session(s(&reference["storage_key"]).into())));}
        doc.text(format!("Started / earliest captured: {}",insights::when(&start)),TEXT);
        doc.text(format!("Last captured event: {}",insights::when(&last)),TEXT);
        let mut models:Vec<_>=arr(&session["events"]).iter().filter_map(|e|e["model"].as_str()).filter(|m|!m.is_empty()).collect();models.sort();models.dedup();
        doc.text(format!("Models: {} · {} events",if models.is_empty(){"unknown".into()}else{models.join(", ")},arr(&session["events"]).len()),MUTED);
    }
    if sessions.is_empty(){doc.text("No captured sessions. /coverage explains discovery and exclusions.",AMBER);}
    if !doc.sources.is_empty(){doc.source_selection=Some(0);}
    doc.notice=Some(("Select a session and press Enter · sessions remain separate across tools".into(),MUTED));doc
}
pub(super) fn session(session:&Value)->Document{
    let mut doc=Document::new(View::Session,format!("{} · {}",s(&session["agent"]),s(&session["id"])));
    let (start,last)=insights::session_dates(session);
    doc.text(format!("Started / earliest captured: {}",insights::when(&start)),MUTED);
    doc.text(format!("Last captured event: {}",insights::when(&last)),MUTED);
    doc.text("Historical transcript · original source position for JSONL; normalized position for SQLite.",AMBER);
    for warning in arr(&session["warnings"]){doc.text(s(warning),AMBER);}
    let events=arr(&session["events"]);let start=events.len().saturating_sub(500);
    if start>0{doc.text(format!("Showing last 500 events; {start} earlier events omitted from this view."),AMBER);}
    for e in &events[start..]{event(&mut doc,e,s(&session["agent"]),2400);}
    doc
}
fn event(doc:&mut Document,e:&Value,agent:&str,limit:usize){
    super::document::conversation(doc,e,agent,limit);

}
pub(super) fn context(review:&Value,sessions:&[Value],target:Target,decisions:bool)->Document{
    let mut doc=Document::new(if decisions{View::Decisions}else{View::Timeline},format!("{} · {}",if decisions{"Decision context"}else{"History"},target.selector()));
    captured(&mut doc,review);
    doc.text("Grouped by recorded user turn, oldest first. File edits or explicit file mentions establish relevance, not causation. Symbol filtering uses explicit text/record matches, not semantic attribution.",MUTED);
    let items=insights::timeline(Path::new(s(&review["root"])),sessions,&target.file,target.symbol.as_deref());
    if items.is_empty(){doc.heading("No captured context matches this file/symbol");doc.text("Original intent unknown. Try the whole file, /coverage, or ask for an explicitly inferred explanation with /why.",AMBER);}
    let start=items.len().saturating_sub(40);
    if start>0{doc.text(format!("Showing latest 40 turns; {start} older turns omitted."),AMBER);}
    for (index,item) in items.iter().enumerate().skip(start){
        doc.heading(format!("Turn {} · {} · Open session ›",index+1,insights::local_date(&item["timestamp"])));
        if let Some(reference)=arr(&review["sessions"]).iter().find(|r|r["agent"]==item["agent"]&&r["id"]==item["session_id"]){doc.sources.push((doc.lines.len()-1,Link::Session(s(&reference["storage_key"]).into())));}
        doc.text(format!("Linkage: {} · {} recorded edits",s(&item["match"]),arr(&item["edits"]).len()),MUTED);
        if items[index+1..].iter().any(|later|!arr(&later["edits"]).is_empty()){
            doc.text("Later edits to this file are captured below. This context may no longer apply; no supersession is inferred automatically.",AMBER);
        }
        if decisions{
            if arr(&item["records"]).is_empty(){doc.text("No structured decision record in this turn. Alternatives, assumptions, and intended tradeoffs may be unknown.",AMBER);}
            for r in arr(&item["records"]){let record=&r["record"];
                doc.heading("Recorded decision · self-reported");
                for field in ["symbol","decision","reason","requirement","timing","validation"]{doc.text(format!("{field}: {}",record[field].as_str().unwrap_or("unknown / not recorded")),TEXT);}
                for field in ["alternatives","tradeoffs","evidence","related_edits"]{let values:Vec<_>=arr(&record[field]).iter().filter_map(Value::as_str).collect();doc.text(format!("{field}: {}",if values.is_empty(){"unknown / not recorded".into()}else{values.join("; ")}),MUTED);}
                doc.text(format!("Recorded at event {}. Any referenced evidence still needs verification.",s(&r["event_id"])),MUTED);
            }
            doc.heading("Review and validation");
            if arr(&item["validation"]).is_empty(){doc.text("Validation gap: no test command captured in this turn. Tests may have run elsewhere.",AMBER);}
            for test in arr(&item["validation"]){doc.text(format!("{} · {}",s(&test["event_id"]),s(&test["outcome"])),TEXT);doc.text(crate::security::short(s(&test["command"]),1000),MUTED);}
            doc.text("Reviewer checks (proposed, not agent intent): confirm the requirement, boundary responsibilities, failure handling, and regression coverage.",MUTED);
        }
        doc.heading(if decisions{"Supporting captured context"}else{"Captured events"});
        let events=arr(&item["events"]);
        for e in events.iter().take(60){event(&mut doc,e,s(&item["agent"]),if decisions{1000}else{1600});}
        if events.len()>60{doc.text(format!("{} more events omitted; open the session to inspect.",events.len()-60),AMBER);}
    }
    doc.target=Some(target);doc
}
pub(super) fn export(review:&Value,sessions:&[Value])->Document{
    let text=insights::brief(review,sessions);let mut doc=Document::new(View::Export,"Export preview · review before sharing");
    doc.text("Review for sensitive content. /export save writes this exact brief to .wy; nothing is uploaded. Redaction is best-effort.",AMBER);
    doc.text(&text,TEXT);
    doc.artifact=Some(Arc::new(json!({"export_text":text,"review_id":review["id"]})));doc
}
pub(super) fn setup()->Document{
    let mut doc=Document::new(View::Setup,"Optional decision capture instructions");
    doc.text("/setup save writes a template to .wy/decision-instructions.md. It does NOT alter agent configuration. Copy it into your chosen agent's project instructions after reviewing it.",AMBER);
    doc.text(insights::DECISION_INSTRUCTIONS,TEXT);doc
}
