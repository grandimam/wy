//! Offline history navigation: preserve agent/session boundaries and visible dates.
use super::{Target, document::{Document, View, Link}, theme::*};
use crate::{arr, s, n, insights, history};
use serde_json::{Value,json};
use std::{path::Path,sync::Arc};

pub(super) const EVENTS_PER_PAGE:usize=20;
#[derive(Clone)]
pub(super) enum Page {
    History{items:Arc<Vec<Value>>,target:Target,decisions:bool,index:usize},
    Session{session:Arc<Value>,index:usize},
    Work{work:Arc<Value>,index:usize},
}
pub(super) fn page(review:&Value,state:&Page,index:usize)->Document {
    match state {
        Page::History{items,target,decisions,..}=>context_page(review,items.clone(),target.clone(),*decisions,index),
        Page::Session{session,..}=>session_page(session.clone(),index),
        Page::Work{work,..}=>super::session_views::flow(work.clone(),index),
    }
}
impl Page {pub fn index(&self)->usize{match self{Self::History{index,..}|Self::Session{index,..}|Self::Work{index,..}=>*index}}}
fn pager(doc:&mut Document,index:usize,count:usize){
    doc.text(format!("Page {} of {} · up to {EVENTS_PER_PAGE} events",index+1,count.max(1)),TEXT);
    if index>0 {
        doc.sources.push((doc.lines.len(),Link::Page(index-1)));
        doc.text("[ ← Previous page ]",ACCENT);
    }
    if index+1<count {
        doc.sources.push((doc.lines.len(),Link::Page(index+1)));
        doc.text("[ Next page → ]",ACCENT);
    }
}
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
#[cfg(test)]
pub(super) fn session(session:&Value)->Document{
    session_page(Arc::new(session.clone()),0)
}
fn session_page(session:Arc<Value>,index:usize)->Document{
    let count=arr(&session["events"]).len().div_ceil(EVENTS_PER_PAGE).max(1);
    let index=index.min(count-1);
    let mut doc=Document::new(View::Session,format!("{} · {}",s(&session["agent"]),s(&session["id"])));
    pager(&mut doc,index,count);
    let (start,last)=insights::session_dates(&session);
    doc.text(format!("Started / earliest captured: {}",insights::when(&start)),MUTED);
    doc.text(format!("Last captured event: {}",insights::when(&last)),MUTED);
    doc.text("Historical transcript · original source position for JSONL; normalized position for SQLite.",AMBER);
    for warning in arr(&session["warnings"]){doc.text(s(warning),AMBER);}
    let events=arr(&session["events"]);
    let end=events.len().saturating_sub(index*EVENTS_PER_PAGE);let start=end.saturating_sub(EVENTS_PER_PAGE);
    doc.text("Latest page first; messages on each page remain in recorded order.",TEXT);
    for e in &events[start..end]{event(&mut doc,e,s(&session["agent"]),2400);}
    if count>1{pager(&mut doc,index,count);}
    doc.pagination=Some(Page::Session{session,index});
    doc
}
fn event(doc:&mut Document,e:&Value,agent:&str,limit:usize){
    super::document::conversation(doc,e,agent,limit);

}
pub(super) fn context(review:&Value,sessions:&[Value],target:Target,decisions:bool)->Document{
    let items=Arc::new(insights::timeline(Path::new(s(&review["root"])),sessions,&target.file,target.symbol.as_deref()));
    context_page(review,items,target,decisions,0)
}
fn context_page(review:&Value,items:Arc<Vec<Value>>,target:Target,decisions:bool,page_index:usize)->Document{
    // Page descriptors are just indices. Do not copy turn/event JSON when paging.
    let slots:Vec<_>=items.iter().enumerate().rev().flat_map(|(i,item)|{
        (0..arr(&item["events"]).len().div_ceil(EVENTS_PER_PAGE).max(1)).map(move|part|(i,part))
    }).collect();
    let count=slots.len().max(1);let page_index=page_index.min(count-1);
    let mut doc=Document::new(if decisions{View::Decisions}else{View::Timeline},format!("{} · {}",if decisions{"Decision context"}else{"History"},target.selector()));
    pager(&mut doc,page_index,count);
    doc.text("Conversations newest first · messages within a conversation stay in order",TEXT);
    if items.is_empty(){doc.heading("No captured context matches this file/symbol");doc.text("Try the whole file or /coverage to inspect capture gaps.",TEXT);}
    for &(index,part) in slots.get(page_index).into_iter(){
        let item=&items[index];
        doc.heading(format!("Turn {} · {} · Open session ›",index+1,insights::local_date(&item["timestamp"])));
        if let Some(reference)=arr(&review["sessions"]).iter().find(|r|r["agent"]==item["agent"]&&r["id"]==item["session_id"]){doc.sources.push((doc.lines.len()-1,Link::Session(s(&reference["storage_key"]).into())));}
        doc.text(format!("Linkage: {} · {} recorded edits",s(&item["match"]),arr(&item["edits"]).len()),MUTED);
        if items[index+1..].iter().any(|later|!arr(&later["edits"]).is_empty()){
            doc.text("Newer edits to this file are also captured. This earlier context may no longer apply.",AMBER);
        }
        if decisions{
            if arr(&item["records"]).is_empty(){doc.text("No structured decision record in this turn. Alternatives, assumptions, and intended tradeoffs may be unknown.",AMBER);}
            for r in arr(&item["records"]).iter().take(8){let record=&r["record"];
                doc.heading("Recorded decision · self-reported");
                for field in ["symbol","decision","reason","requirement","timing","validation"]{doc.text(format!("{field}: {}",record[field].as_str().unwrap_or("unknown / not recorded")),TEXT);}
                for field in ["alternatives","tradeoffs","evidence","related_edits"]{let values:Vec<_>=arr(&record[field]).iter().filter_map(Value::as_str).collect();doc.text(format!("{field}: {}",if values.is_empty(){"unknown / not recorded".into()}else{values.join("; ")}),MUTED);}
                doc.text(format!("Recorded at event {}. Any referenced evidence still needs verification.",s(&r["event_id"])),MUTED);
            }
            doc.heading("Review and validation");
            if arr(&item["validation"]).is_empty(){doc.text("Validation gap: no test command captured in this turn. Tests may have run elsewhere.",AMBER);}
            for test in arr(&item["validation"]).iter().take(8){doc.text(format!("{} · {}",s(&test["event_id"]),s(&test["outcome"])),TEXT);doc.text(crate::security::short(s(&test["command"]),1000),MUTED);}
            doc.text("Reviewer checks (proposed, not agent intent): confirm the requirement, boundary responsibilities, failure handling, and regression coverage.",MUTED);
        }
        doc.heading(if decisions{"Supporting captured context"}else{"Captured events"});
        let events=arr(&item["events"]);
        let start=part*EVENTS_PER_PAGE;let end=(start+EVENTS_PER_PAGE).min(events.len());
        doc.text(format!("Events {}–{} of {} in this turn",if events.is_empty(){0}else{start+1},end,events.len()),TEXT);
        for e in &events[start..end]{event(&mut doc,e,s(&item["agent"]),if decisions{1000}else{1600});}
        if decisions && (arr(&item["records"]).len()>8||arr(&item["validation"]).len()>8){doc.text("Decision/test overview limited to 8 records each; page through captured events to inspect the rest.",TEXT);}
    }
    if count>1{pager(&mut doc,page_index,count);}
    doc.target=Some(target.clone());
    doc.pagination=Some(Page::History{items,target,decisions,index:page_index});doc
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
