use super::{Target, theme::*};
use crate::{arr, history::attribution, n, presentation, s};
use ratatui::prelude::*;
use serde_json::Value;
use std::{sync::Arc, collections::{HashMap, HashSet}};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum View {
    Empty,
    DecisionOverview,
    DecisionDetail,
    SessionWork,
    SessionImplementation,
    SessionComparison,
    Diff,
    SessionCode,
    Recorded,
    Explanation,
    Evidence,
    Commits,
    Commit,
    Original,
    Turn,
    Help,
    Coverage,
    Sessions,
    Session,
    Timeline,
    Decisions,
    Export,
    Setup,
}
/// What a clickable or selectable line opens.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Link {
    Page(usize),
    DecisionHome,
    Decision(usize),
    DiscoverDecisions,
    SessionPicker,
    SessionPage(usize),
    SessionEdit(Value),
    CompareSessionEdit(Value),
    Explain,
    Disclosure(String),
    Session(String),
    /// A numbered source of the current document.
    Source(usize),
    /// The conversation turn that recorded an edit.
    Turn(Value),
    /// Scroll the reader to a line (a reason's first change, or back to the reason).
    Line(usize),
}
#[derive(Clone)]
pub(super) struct Document {
    pub kind: View,
    pub title: String,
    pub lines: Vec<Line<'static>>,
    pub target: Option<Target>,
    pub artifact: Option<Arc<Value>>,
    pub session_edit: Option<Value>,
    pub sources: Vec<(usize, Link)>,
    pub source_selection: Option<usize>,
    pub scroll: u16,
    pub horizontal: u16,
    pub notice: Option<(String, Color)>,
    pub commits: Vec<String>,
    pub originals: Vec<Value>,
    disclosures: HashMap<String, (String, Vec<Line<'static>>, HashMap<usize,usize>)>,
    disclosure_links: HashMap<String, Vec<(usize, Link)>>,
    pub expanded: HashSet<String>,
    pub pagination: Option<super::history_views::Page>,
    /// Code rows keep a fixed gutter and pan independently of prose.
    pub code_gutters: HashMap<usize,usize>,
}
impl Document {
    pub fn new(kind: View, title: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            lines: vec![],
            target: None,
            artifact: None,
            session_edit: None,
            sources: vec![],
            source_selection: None,
            scroll: 0,
            horizontal: 0,
            notice: None,
            commits: vec![],
            originals: vec![],
            disclosures: HashMap::new(),
            disclosure_links: HashMap::new(),
            expanded: HashSet::new(),
            pagination: None,
            code_gutters: HashMap::new(),
        }
    }
    pub fn disclosure(&mut self, id: String, title: &str, lines: Vec<Line<'static>>) {
        self.sources.push((self.lines.len(),Link::Disclosure(id.clone())));
        self.text(format!("{}▶ {}",&title[..title.len()-title.trim_start().len()],title.trim_start()),ACCENT);
        self.disclosures.insert(id,(title.into(),lines,HashMap::new()));
    }
    /// Links use body-relative rows and exist only while this disclosure is open.
    pub fn linked_disclosure(&mut self,id:String,title:&str,lines:Vec<Line<'static>>,links:Vec<(usize,Link)>){
        assert!(links.iter().all(|(row,_)|*row<lines.len()));
        self.disclosure(id.clone(),title,lines);
        self.disclosure_links.insert(id,links);
    }
    /// Fold a newly rendered code block, preserving its fixed gutters on reopen.
    pub fn fold_code(&mut self,id:String,title:&str,start:usize,open:bool){
        let body=self.lines.split_off(start);
        let gutters=self.code_gutters.iter().filter(|(line,_)|**line>=start).map(|(line,width)|(line-start,*width)).collect();
        self.code_gutters.retain(|line,_|*line<start);
        self.disclosure(id.clone(),title,body);
        self.disclosures.get_mut(&id).unwrap().2=gutters;
        if open{self.toggle_disclosure(&id);}
    }
    pub fn toggle_disclosure(&mut self, id: &str) {
        let Some((title,body,gutters))=self.disclosures.get(id) else{return};
        let Some(at)=self.sources.iter().find_map(|(line,link)|(*link==Link::Disclosure(id.into())).then_some(*line)) else{return};
        let opening=!self.expanded.contains(id);
        let selected=self.source_selection.and_then(|i|self.sources.get(i)).cloned();
        let count=body.len();let pivot=at+1;
        if opening {self.lines.splice(pivot..pivot,body.clone());self.expanded.insert(id.into());}
        else {
            self.lines.drain(pivot..pivot+count);self.expanded.remove(id);
            self.code_gutters.retain(|line,_|*line<pivot||*line>=pivot+count);
            self.sources.retain(|(line,_)|*line<pivot||*line>=pivot+count);
        }
        self.lines[at]=Line::styled(format!("{}{} {}",&title[..title.len()-title.trim_start().len()],if opening{"▼"}else{"▶"},title.trim_start()),Style::default().fg(ACCENT));
        let shift=|line:usize|if line<pivot{line}else if opening{line+count}else{line.saturating_sub(count).max(at)};
        self.code_gutters=self.code_gutters.drain().map(|(line,width)|(shift(line),width)).collect();
        if opening{self.code_gutters.extend(gutters.iter().map(|(line,width)|(pivot+line,*width)));}
        for (line,link) in &mut self.sources {
            *line=shift(*line);
            if let Link::Line(target)=link {*target=shift(*target);}
        }
        if opening {
            if let Some(links)=self.disclosure_links.get(id){self.sources.extend(links.iter().map(|(row,link)|(pivot+row,link.clone())));}
        }
        self.sources.sort_by_key(|(row,_)|*row);
        self.source_selection=selected.and_then(|(row,mut link)|{
            if !opening && (pivot..pivot+count).contains(&row){link=Link::Disclosure(id.into());}
            if let Link::Line(target)=&mut link{*target=shift(*target);}
            self.sources.iter().position(|(line,candidate)|*line==shift(row) && *candidate==link)
        });
    }
    /// Inclusive logical-row bounds; Ratatui sizes panels to the current viewport.
    pub fn reader_panels(&self)->Vec<(usize,usize,bool)>{
        self.sources.iter().filter_map(|(row,link)|{
            let Link::Disclosure(id)=link else{return None};
            let request=id.starts_with("reason-")&&id.ends_with("-request");
            if !request&&!id.starts_with("code-block-"){return None;}
            let (_,body,_)=self.disclosures.get(id)?;
            Some((*row,*row+if self.expanded.contains(id){body.len()}else{0},request))
        }).collect()
    }
    pub fn explain_button(&mut self) {
        self.gap();
        self.sources.push((self.lines.len(),Link::Explain));
        self.lines.push(Line::styled("  Ask AI to explain this change  ",Style::default().fg(ACCENT).add_modifier(Modifier::REVERSED|Modifier::BOLD)));
        self.text("Click or select and press Enter · e is the shortcut",TEXT);
    }
    pub fn code_line(&mut self,gutter:String,body:&str,color:Color){
        self.code_gutters.insert(self.lines.len(),Line::from(gutter.as_str()).width());
        self.lines.push(Line::from(vec![Span::styled(gutter,Style::default().fg(color)),Span::styled(body.replace('\t',"    "),Style::default().fg(color))]));
    }
    pub fn code(&self) -> bool {
        matches!(self.kind, View::Diff | View::SessionCode | View::SessionImplementation | View::SessionComparison)
    }
    pub fn historical(&self) -> bool {
        matches!(self.kind, View::SessionWork | View::SessionImplementation | View::SessionComparison | View::DecisionOverview | View::DecisionDetail | View::Commits | View::Commit | View::Original | View::Turn | View::Coverage | View::Sessions | View::Session | View::Timeline | View::Decisions | View::Export | View::Setup)
    }
    pub fn text(&mut self, text: impl AsRef<str>, color: Color) {
        self.lines.extend(
            text.as_ref()
                .lines()
                .map(|l| Line::styled(l.to_owned(), Style::default().fg(color))),
        );
    }
    pub fn gap(&mut self) {
        self.lines.push(Line::default());
    }
    pub fn heading(&mut self, text: impl Into<String>) {
        if self.lines.last().is_some_and(|line| !line.spans.is_empty()) {
            self.gap();
        }
        self.lines.push(Line::styled(
            text.into(),
            Style::default().fg(ACCENT).bold(),
        ));
    }
}


pub(super) fn empty(review: &Value) -> Document {
    let mut doc = Document::new(View::Empty, "Changes");
    doc.text(format!("Captured: {}",crate::insights::when(&review["created_at"])),MUTED);
    doc.text("/coverage explains missing history · /sessions browses dated captures",MUTED);
    if super::explorer::files(review).is_empty() {
        doc.heading("No changed files");
        doc.text("No working-tree diff or recent session code. After your agent edits code, press r to refresh.", MUTED);
    } else {
        doc.heading("Select a changed file");
        doc.text(
            "Its changes and recorded agent notes appear together.",
            MUTED,
        );
    }
    doc
}

pub(super) fn recent_edit<'a>(review: &'a Value, file: &str) -> Option<&'a Value> {
    arr(&review["recent_code"])
        .iter()
        .find(|c| c["file"] == file)
}
pub(super) fn preview(review: &Value, target: Target) -> Document {
    if !arr(&review["changes"])
        .iter()
        .any(|c| c["file"] == target.file)
    {
        if let Some(code) = session_code(review, target.clone()) {
            return code;
        }
    }
    diff(review, target)
}
pub(super) fn session_code(review: &Value, target: Target) -> Option<Document> {
    let edit = recent_edit(review, &target.file)?;
    Some(recorded_code(edit))
}
pub(super) fn recorded_code(edit: &Value) -> Document {
    let mut doc = Document::new(View::SessionCode, s(&edit["file"]));
    let date = crate::insights::when(&edit["timestamp"]);
    doc.text(format!("{} · {date}", s(&edit["agent"])), ACCENT);
    doc.text(
        format!(
            "Session {} · {}",
            s(&edit["session_id"]),
            s(&edit["event_id"])
        ),
        MUTED,
    );
    doc.text(
        if edit["state"] == "applied" {
            "Tool reported success."
        } else {
            "Recorded tool input · execution not confirmed."
        },
        MUTED,
    );
    doc.gap();
    if s(&edit["text"]).is_empty() {
        doc.text(
            "This session recorded a file change without a code excerpt.",
            AMBER,
        );
    }
    super::code_view::render(&mut doc,s(&edit["text"]),s(&edit["format"]),true,usize::MAX);
    if edit["truncated"] == true {
        doc.text("Recorded code truncated at the capture limit.", AMBER);
    }
    doc.notice = Some((
        "Recorded session edit · may differ from current files".into(),
        AMBER,
    ));
    doc.target = Some(Target {
        file: s(&edit["file"]).into(),
        symbol: None,
        line: 1,
    });
    doc.session_edit = Some(crate::history::edit_ref(edit));
    doc
}

pub(super) fn diff(review: &Value, target: Target) -> Document {
    let mut doc = Document::new(View::Diff, target.label());
    if let Some(change) = arr(&review["changes"])
        .iter()
        .find(|c| c["file"] == target.file)
    {
        // File headers repeat the title; the reader starts at the first hunk.
        let skipped = diff_header(s(&change["diff"]));
        for line in s(&change["diff"]).lines().skip(skipped) {
            let style = if line.starts_with('+') {
                Style::default().fg(GREEN)
            } else if line.starts_with('-') {
                Style::default().fg(RED)
            } else if line.starts_with("@@") {
                Style::default().fg(ACCENT)
            } else {
                Style::default().fg(TEXT)
            };
            doc.lines
                .push(Line::styled(line.replace('\t', "    "), style));
        }
        if target.symbol.is_some() {
            let end = arr(&change["symbols"])
                .iter()
                .find(|symbol| n(&symbol["start_line"]) == target.line)
                .map(|symbol| n(&symbol["end_line"]))
                .unwrap_or(target.line);
            if let Some(index) = s(&change["diff"]).lines().position(|line| {
                if !line.starts_with("@@ ") {
                    return false;
                }
                let Some(range) = line
                    .split_whitespace()
                    .nth(2)
                    .and_then(|r| r.strip_prefix('+'))
                else {
                    return false;
                };
                let (start, count) = range.split_once(',').unwrap_or((range, "1"));
                let start = start.parse::<usize>().unwrap_or(0);
                let count = count.parse::<usize>().unwrap_or(1);
                start <= end && start.saturating_add(count.max(1) - 1) >= target.line
            }) {
                doc.scroll = index.saturating_sub(skipped).min(u16::MAX as usize) as u16;
            }
        }
        if change["diff_truncated"] == true {
            doc.text("Diff truncated at the review's capture limit.", AMBER);
        }
    } else {
        doc.text(
            "No captured diff for this file. Press r to refresh changed files.",
            MUTED,
        );
    }
    doc.target = Some(target);
    doc
}
pub(super) fn is_recorded(artifact: &Value) -> bool {
    artifact["context"] == "recorded_session"
}
pub(super) fn citations(artifact: &Value) -> Vec<Value> {
    if is_recorded(artifact) || artifact["context"]=="decision_brief" {
        arr(&artifact["packet"]["evidence"]).to_vec()
    } else {
        presentation::citations(artifact)
    }
}
/// Request-led reader: the saved user request, then changes, then optional agent
/// context. Matching does not establish that a request caused a particular edit.
pub(super) fn recorded(
    review: &Value,
    sessions: &[Value],
    code: &Document,
    brief: bool,
) -> Document {
    let target = code.target.clone().expect("code has a target");
    let notes = crate::history::notes(
        review,
        sessions,
        &target.file,
        code.session_edit.as_ref(),
    );
    let focus = target.symbol.as_ref().map(|symbol| serde_json::json!({"target":target.selector(),"file":target.file,"symbol":symbol,"start_line":target.line}));
    let artifact = Arc::new(
        serde_json::json!({"context":"recorded_session","review_id":review["id"],
        "packet":{"focus_file":target.file,"focus_target":focus,"focus_session_edit":code.session_edit,
            "evidence":notes["evidence"],"note_refs":notes["note_refs"],"gaps":notes["gaps"]}}),
    );
    let mut doc = Document::new(View::Recorded, target.label());
    doc.text(format!("History refreshed {} · r refreshes",crate::insights::capture_age(&review["created_at"])),MUTED);
    doc.gap();
    doc.target = Some(target.clone());
    doc.session_edit = code.session_edit.clone();
    let root = std::path::Path::new(s(&review["root"]));
    let diff = change_diff(review, &target.file);
    if diff.is_none() {
        doc.notice = Some((
            "No current Git changes · showing code saved in an earlier agent session".into(),
            AMBER,
        ));
    }
    let mut found = attribution::reasons(root, sessions, &target.file, diff);
    // A recorded edit kept from an earlier review may no longer have its session loaded.
    if diff.is_none() && arr(&found["hunks"]).is_empty() {
        if let Some(edit) = recent_edit(review, &target.file) {
            let (added, removed) = attribution::edit_size(edit);
            found["hunks"] = serde_json::json!([{"id":edit["id"],"label":"recorded edit","text":edit["text"],
                "format":edit["format"],"added":added,"removed":removed,"status":"recorded","reason":null,"first":false}]);
        }
    }
    let reasons = arr(&found["reasons"]);
    let hunks = arr(&found["hunks"]);
    // Without a matching edit, fall back to conversation that mentions this file.
    let evidence = arr(&notes["evidence"]);
    if reasons.is_empty() {
        doc.text("Original request not captured for these changes.",TEXT);
        doc.heading("CODE CHANGES");
    }
    let mut request_positions:HashMap<String,usize>=HashMap::new();
    let mut first_lines = vec![0; reasons.len()];
    let mut compactions_seen = 0;
    for hunk in hunks {
        if let Some(index) = hunk["reason"].as_u64().map(|i| i as usize) {
            if hunk["first"] == true {
                // Mark where the agent's context was compacted between reasons.
                let compactions = n(&reasons[index]["compactions"]);
                if compactions > compactions_seen {
                    doc.gap();
                    doc.lines.push(Line::from(vec![
                        Span::styled(" ⚠ CONTEXT COMPACTED ", Style::default().fg(AMBER).add_modifier(Modifier::BOLD | Modifier::REVERSED)),
                        Span::styled("  earlier context was summarized", Style::default().fg(AMBER).add_modifier(Modifier::BOLD)),
                    ]));
                    doc.text("Some earlier context may have been replaced by a summary. Summary text is secondary evidence, not proof of original intent.", AMBER);
                    doc.gap();
                }
                compactions_seen = compactions_seen.max(compactions);
                first_lines[index] = doc.lines.len();
                if let Some((request,earlier))=primary_request(&reasons[index]) {
                    let key=format!("{}:{}:{}",s(&reasons[index]["agent"]),s(&reasons[index]["session_id"]),s(&request["id"]));
                    if let Some(line)=request_positions.get(&key) {
                        doc.sources.push((doc.lines.len(),Link::Line(*line)));
                        doc.text(if earlier{"↑ Earlier request above · context only"}else{"↑ Your request above"},ACCENT);
                    } else {
                        request_positions.insert(key,doc.lines.len());
                        request_block(&mut doc,index+1,&reasons[index],request,earlier,!brief);
                    }
                } else {
                    doc.text("Original request not captured for this change.",TEXT);
                    if let Some(text)=reasons[index]["request"]["text"].as_str(){
                        let mut lines=vec![];quoted(&mut lines,text,1200,TEXT);
                        doc.disclosure(format!("reason-{}-confirmation",index+1),"Saved follow-up · request context unavailable",lines);
                    }
                }
                doc.gap();
            } else {
                doc.sources.push((doc.lines.len(), Link::Line(first_lines[index])));
                doc.lines.push(Line::from(vec![
                    Span::styled("↑ ", Style::default().fg(ACCENT)),
                    badge(index + 1),
                    Span::styled(" same message as above", Style::default().fg(MUTED)),
                ]));
            }
        }
        if !reasons.is_empty()&&hunk["reason"].is_null(){doc.text("Original request not captured for this change.",TEXT);}
        change(&mut doc, hunk, reasons.get(hunk["reason"].as_u64().unwrap_or(u64::MAX) as usize));
        if hunk["first"]==true {
            if let Some(index)=hunk["reason"].as_u64().map(|i|i as usize){why(&mut doc,index+1,&reasons[index],brief);}
        }
        let reason=reasons.get(hunk["reason"].as_u64().unwrap_or(u64::MAX) as usize);
        if let Some(edit)=reason.map(|r|&r["edit"]).filter(|e|e.is_object()){
            doc.sources.push((doc.lines.len(),Link::Turn(edit.clone())));
            doc.text("[ View original chat ]",ACCENT);
        }
        super::code_view::technical_details(&mut doc,s(&hunk["text"]),s(&hunk["format"]),reason.map(session_details).unwrap_or_default());
        doc.gap();
    }
    if hunks.is_empty() {
        doc.text("No changes recorded for this file.", MUTED);
    }
    if reasons.is_empty() {
        doc.text("No explanation directly linked to this edit.",TEXT);
        if !evidence.is_empty(){related(&mut doc,evidence);}
        let mut details=vec![];
        for gap in arr(&notes["gaps"]){details.push(Line::styled(s(gap).to_owned(),Style::default().fg(TEXT)));}
        if !details.is_empty(){doc.disclosure("capture-gaps".into(),"About missing context",details);}
    }
    doc.explain_button();
    doc.artifact = Some(artifact);
    doc
}
/// Opening the explanation reader never starts a model request; only its button does.
pub(super) fn explanation_prompt(target: Target) -> Document {
    let mut doc=Document::new(View::Explanation,target.label());
    doc.heading("Understand this change");
    doc.text("Ask AI to assess the code and captured history. This creates a new explanation; it does not recover the coding agent's original intent.",TEXT);
    doc.explain_button();
    doc.text("Uses your configured explanation CLI and may consume account usage.",TEXT);
    doc.target=Some(target);
    doc.source_selection=Some(0);
    doc
}
fn change_diff<'a>(review: &'a Value, file: &str) -> Option<&'a str> {
    arr(&review["changes"])
        .iter()
        .find(|c| c["file"] == file)
        .and_then(|c| c["diff"].as_str())
}
/// The reason number as a solid badge, so it stands out from code and prose.
fn badge(number: usize) -> Span<'static> {
    Span::styled(
        format!(" {number} "),
        Style::default()
            .fg(ACCENT)
            .add_modifier(Modifier::REVERSED | Modifier::BOLD),
    )
}
/// Minimal Markdown cleanup for prose display; code is never rendered this way.
fn plain(text: &str) -> String {
    let mut out=String::new();
    for line in text.lines() {
        let trimmed=line.trim_start();
        let line=trimmed.strip_prefix("### ").or_else(||trimmed.strip_prefix("## ")).or_else(||trimmed.strip_prefix("# ")).unwrap_or(line);
        let line=line.replace("**","");
        out.push_str(&line);out.push('\n');
    }
    out.trim_end().to_owned()
}
fn quoted(lines:&mut Vec<Line<'static>>, text:&str, limit:usize, color:Color){
    let short=crate::security::short(&plain(text),limit);
    for line in short.lines(){lines.push(Line::from(vec![Span::styled("  │ ",Style::default().fg(color)),Span::styled(line.to_owned(),Style::default().fg(TEXT))]));}
    if plain(text).chars().count()>limit {lines.push(Line::styled("  [shortened · open the conversation for the full text]",Style::default().fg(AMBER)));}
}
fn primary_request(reason:&Value)->Option<(&Value,bool)>{
    let request=&reason["request"];
    if request["kind"]!="user"||!crate::history::provenance::original(request)||s(&request["text"]).trim().is_empty(){return None;}
    if attribution::confirmation(s(&request["text"])){
        let prior=&reason["prior_request"];
        (prior["kind"]=="user"&&crate::history::provenance::original(prior)&&!s(&prior["text"]).trim().is_empty()&&!attribution::confirmation(s(&prior["text"]))).then_some((prior,true))
    }else{Some((request,false))}
}
fn request_block(doc:&mut Document,number:usize,reason:&Value,request:&Value,earlier:bool,open:bool){
    let mut lines=vec![];
    lines.push(Line::default());
    lines.extend(crate::security::short(&plain(s(&request["text"])),16000).lines().map(|line|Line::styled(line.to_owned(),Style::default().fg(TEXT))));
    if s(&request["text"]).chars().count()>16000{lines.push(Line::styled("[Request excerpt shortened]",Style::default().fg(AMBER)));}
    lines.push(Line::default());
    if earlier{lines.push(Line::styled("Earlier request in this session · context only, not a confirmed link.",Style::default().fg(TEXT)));}
    lines.push(Line::styled(crate::insights::local_date(&request["timestamp"]),Style::default().fg(TEXT)));
    if earlier{lines.push(Line::styled(format!("Follow-up that started this turn: {}",crate::security::short(s(&reason["request"]["text"]),200)),Style::default().fg(TEXT)));}
    // A final empty row is the card's bottom border; body text wraps inside it.
    lines.push(Line::default());
    let id=format!("reason-{number}-request");
    doc.disclosure(id.clone(),"YOUR REQUEST",lines);
    if open{doc.toggle_disclosure(&id);}
}
/// Agent response and working notes remain secondary, below the changed code.
fn why(doc: &mut Document, number: usize, reason: &Value, brief: bool) {
    let message = &reason["message"];
    let text = plain(s(&message["text"]));
    let decision=crate::insights::decision(message);
    let original=message.is_object()&&crate::history::provenance::original(message);
    let (label,color)=if decision.is_some(){("RECORDED DECISION",GREEN)}else if text.is_empty(){("NO AGENT MESSAGE BEFORE THIS EDIT",AMBER)}else if original{("AGENT MESSAGE",GREEN)}else{("UNVERIFIED CONTEXT",AMBER)};
    let id=|name:&str|format!("reason-{number}-{name}");
    if text.is_empty(){doc.text("No explanation was saved with this change.",TEXT);}
    else {
        let mut lines=vec![Line::styled(label,Style::default().fg(color).bold())];
        quoted(&mut lines,&text,16000,color);
        lines.push(Line::styled(crate::insights::local_date(&reason["edit"]["timestamp"]),Style::default().fg(TEXT)));
        doc.disclosure(id("message"),if decision.is_some(){"Why the agent chose this"}else{"Agent response"},lines);
    }
    if brief {doc.gap();return;}
    if let Some(rationale)=reason["rationale"]["text"].as_str(){
        let mut lines=vec![Line::styled("  These notes may include ideas the agent did not use.",Style::default().fg(AMBER))];
        quoted(&mut lines,rationale,1200,AMBER);
        doc.disclosure(id("rationale"),"Agent notes",lines);
    }
    doc.gap();
}
fn session_details(reason:&Value)->Vec<Line<'static>>{
    let decision=crate::insights::decision(&reason["message"]);
    let original=reason["message"].is_object()&&crate::history::provenance::original(&reason["message"]);
    vec![
        Line::styled(format!("  Coding tool: {}",s(&reason["agent"])),Style::default().fg(TEXT)),
        Line::styled(format!("  Model: {}",reason["model"].as_str().unwrap_or("unknown")),Style::default().fg(TEXT)),
        Line::styled(format!("  Session: {}",s(&reason["session_id"])),Style::default().fg(TEXT)),
        Line::styled(if decision.is_some(){"  This decision is the agent's own explanation, not independent verification."}else if original{"  Message linked through a matching recorded edit."}else{"  Source could not be verified as an original message."},Style::default().fg(TEXT)),
        Line::styled(format!("  Edit recorded {}",crate::insights::when(&reason["edit"]["timestamp"])),Style::default().fg(TEXT)),
        Line::styled("  Matching text links the message to this edit; it does not establish intent.",Style::default().fg(TEXT)),
        Line::styled("  View original chat shows saved messages, not a new AI conversation.",Style::default().fg(TEXT)),
        Line::default(),
    ]
}
/// Stable, literal labels: colour is supplemental, never the only distinction.
pub(super) fn conversation_role(event: &Value) -> (&'static str, Color, &'static str) {
    let kind=event["role"].as_str().unwrap_or(s(&event["kind"]));
    match kind {
        "rationale" => ("AGENT NOTES", TEXT, "These notes may include ideas the agent did not use."),
        "summary" => ("SESSION SUMMARY", TEXT, "A condensed account of earlier work, not the original messages."),
        "tool_output" => ("TOOL RESULT", TEXT, "Recorded output · not an agent explanation"),
        "read"|"search"|"tool_call"|"change"|"test" => ("TOOL ACTION", TEXT, "Recorded operation · execution may need confirmation"),
        "user" if crate::history::provenance::original(event) => ("YOUR REQUEST", ACCENT, "User instruction recorded in this session"),
        "assistant" if crate::history::provenance::original(event) => ("AGENT MESSAGE", GREEN, "A saved response from the coding session, not a new wy explanation."),
        _ => ("UNVERIFIED CONTEXT", AMBER, "Source not verified · do not treat as original intent"),
    }
}
fn event_details(event:&Value,agent:&str)->Vec<Line<'static>> {
    let mut lines=vec![Line::styled(format!("  {}",conversation_role(event).2),Style::default().fg(TEXT))];
    for (name,value) in [("Coding tool",agent),("Model",s(&event["model"])),("Session",s(&event["session_id"])),("Event",event["event_id"].as_str().unwrap_or(s(&event["id"])))] {
        if !value.is_empty(){lines.push(Line::styled(format!("  {name}: {value}"),Style::default().fg(TEXT)));}
    }
    if let Some(message)=event["provenance"]["message_id"].as_str(){lines.push(Line::styled(format!("  Message: {message}"),Style::default().fg(TEXT)));}
    lines.push(Line::styled(format!("  Recorded: {}",crate::insights::when(&event["timestamp"])),Style::default().fg(TEXT)));
    if let Some(status)=crate::history::origins::status(event){lines.push(Line::styled(format!("  {status}"),Style::default().fg(TEXT)));}
    lines
}
pub(super) fn event_body(event:&Value,limit:usize)->Vec<Line<'static>> {
    let mut lines=vec![];
    let (label,color,_)=conversation_role(event);
    if ["TOOL ACTION","TOOL RESULT"].contains(&label) {
        // Preserve literal tool/code content; Markdown cleanup is only for prose.
        for line in crate::security::short(s(&event["text"]),limit).lines(){lines.push(Line::styled(format!("  {line}"),Style::default().fg(TEXT)));}
    } else {quoted(&mut lines,s(&event["text"]),limit,color);}
    if event["truncated"]==true || (["TOOL ACTION","TOOL RESULT"].contains(&label)&&s(&event["text"]).chars().count()>limit){lines.push(Line::styled("  [Excerpt shortened]",Style::default().fg(AMBER)));}
    lines
}
/// The same disclosure-first presentation in every history view.
pub(super) fn conversation(doc: &mut Document, event: &Value, agent: &str, limit: usize) {
    let (label,color,_)=conversation_role(event);
    let id=format!("event-{}",doc.disclosures.len());
    let details=event_details(event,agent);
    doc.gap();
    let kind=event["role"].as_str().unwrap_or(s(&event["kind"]));
    let collapsed=!["user","assistant"].contains(&kind)||!crate::history::provenance::original(event);
    if collapsed {
        let title=match label {"AGENT NOTES"=>"Agent notes","SESSION SUMMARY"=>"Session summary","TOOL ACTION"=>"Tool activity","TOOL RESULT"=>"Tool result",_=>"Unverified source"};
        let mut body=event_body(event,limit);body.push(Line::default());body.extend(details);
        doc.disclosure(id,&format!("{title} · {}{}",crate::insights::local_date(&event["timestamp"]),if event["failed"]==true{" · failed"}else{""}),body);
    } else {
        doc.lines.push(Line::styled(format!("▎ {label}"),Style::default().fg(color).bold()));
        doc.lines.extend(event_body(event,limit));
        doc.text(format!("  {}",crate::insights::local_date(&event["timestamp"])),TEXT);
        doc.disclosure(id,"Session details",details);
    }
    doc.gap();
}
/// File mentions are supporting context, never a replacement for the code diff.
fn related(doc: &mut Document, evidence: &[Value]) {
    let mut lines=vec![Line::styled("These excerpts mention the file, but are not linked to a captured edit.",Style::default().fg(TEXT))];
    for event in evidence {
        lines.push(Line::default());
        lines.push(Line::styled(format!("{} · {}",conversation_role(event).0,crate::insights::local_date(&event["timestamp"])),Style::default().fg(TEXT).bold()));
        lines.extend(event_body(event,1400));
        lines.extend(event_details(event,s(&event["agent"])));
    }
    doc.disclosure("related-conversation".into(),if evidence.iter().all(|e|e["role"]=="rationale"){"Agent notes mentioning this file"}else{"Conversation mentioning this file"},lines);
}
/// One hunk: `@@ line 10 · fn get()  +2 −1`, a status when needed, then its lines.
/// The header opens the turn that made the change when a recorded edit matched.
fn change(doc: &mut Document, hunk: &Value, reason: Option<&Value>) {
    let historical=hunk["status"]=="recorded";
    let label=if historical{format!("Earlier session · {}",reason.map(|r|crate::insights::local_date(&r["edit"]["timestamp"])).unwrap_or_else(||"date unknown".into()))}else{format!("Change · {}",s(&hunk["label"]))};
    let mut spans=vec![];
    if let Some(index)=hunk["reason"].as_u64(){spans.extend([badge(index as usize+1),Span::raw(" ")]);}
    spans.push(Span::styled(label,Style::default().fg(ACCENT).bold()));
    if !historical {spans.push(Span::styled(format!("  +{} −{}",n(&hunk["added"]),n(&hunk["removed"])),Style::default().fg(TEXT)));}
    match s(&hunk["status"]) {
        "partial" => spans.push(Span::styled("  · partial text overlap", Style::default().fg(AMBER))),
        "none" => spans.push(Span::styled("  · no matching edit found", Style::default().fg(MUTED))),
        _ => {}
    }
    doc.lines.push(Line::from(spans));
    super::code_view::render_code(doc,s(&hunk["text"]),s(&hunk["format"]),historical,if historical{80}else{usize::MAX});
    doc.gap();
}
/// Lines before the first hunk (`diff --git`, `---`, `+++`).
fn diff_header(diff: &str) -> usize {
    diff.lines().take_while(|l| !l.starts_with("@@")).count()
}
/// The conversation around one recorded edit, with the edit itself in place.
pub(super) fn turn(session: &Value, edit: &Value) -> Document {
    let mut doc = Document::new(View::Turn, format!("Messages around this change · {}", s(&edit["file"])));
    doc.notice = Some(("Historical conversation · Esc goes back".into(), MUTED));
    let (start,last)=crate::insights::session_dates(session);
    doc.text(format!("{} · session {}",s(&session["agent"]),s(&session["id"])),MUTED);
    doc.text(format!("Session started / earliest captured: {}",crate::insights::when(&start)),MUTED);
    doc.text(format!("Last captured event: {}",crate::insights::when(&last)),MUTED);
    doc.text(format!("Selected edit recorded: {}",crate::insights::when(&edit["timestamp"])),AMBER);
    for event in attribution::turn(session, &edit["event_id"]) {
        if event["id"] == edit["event_id"] {
            let (added, removed) = attribution::edit_size(edit);
            doc.lines.push(Line::from(vec![
                Span::styled("✎ Edit ", Style::default().fg(ACCENT).bold()),
                Span::styled(s(&edit["file"]).to_owned(), Style::default().fg(TEXT)),
                Span::styled(format!("  +{added} −{removed}"), Style::default().fg(MUTED)),
            ]));
            super::code_view::render(&mut doc,s(&edit["text"]),s(&edit["format"]),true,usize::MAX);
            doc.gap();
            continue;
        }
        conversation(&mut doc,&event,s(&session["agent"]),4000);
    }
    if doc.lines.is_empty() {
        doc.text("This turn is no longer in the saved conversation.", AMBER);
    }
    doc.target = Some(Target { file: s(&edit["file"]).into(), symbol: None, line: 1 });
    doc
}
pub(super) fn artifact_target(artifact: &Value) -> Option<Target> {
    let focus = &artifact["packet"]["focus_target"];
    let selector = s(&focus["target"]).rsplit_once(':');
    let file = focus["file"]
        .as_str()
        .or_else(|| selector.map(|(f, _)| f))
        .or_else(|| artifact["packet"]["focus_file"].as_str())?;
    Some(Target {
        file: file.into(),
        symbol: focus["symbol"]
            .as_str()
            .filter(|name| !name.is_empty())
            .map(str::to_owned),
        line: n(&focus["start_line"]).max(1),
    })
}
pub(super) fn explanation(artifact: Arc<Value>) -> Document {
    let mut checked = (*artifact).clone();
    crate::history::provenance::sanitize_artifact(&mut checked);
    let artifact = Arc::new(checked);
    let target = artifact_target(&artifact);
    let title = target
        .as_ref()
        .map(Target::label)
        .unwrap_or_else(|| "All changes".into());
    let mut doc = Document::new(View::Explanation, title);
    doc.text(format!("AI explanation · generated {}",crate::insights::local_date(&artifact["created_at"])),TEXT);
    doc.text("A new assessment of the evidence, not the original coding conversation.",TEXT);
    doc.gap();
    if let Some(warning) = artifact["provenance_warning"].as_str() {
        doc.text(warning, AMBER);
        doc.gap();
    }
    doc.target = target;
    let cited = citations(&artifact);
    let result = &artifact["explanation"];
    if let Some(edit) = artifact["packet"]["focus_session_edit"].as_object() {
        doc.text(
            format!(
                "Recorded session edit · {} · {}",
                s(&edit["agent"]),
                s(&edit["timestamp"])
            ),
            MUTED,
        );
    }
    let judgments = arr(&result["judgments"]);
    let recorded: Vec<_> = judgments
        .iter()
        .filter(|j| j["status"] == "recorded")
        .collect();
    if recorded.is_empty() {
        let has_history = arr(&artifact["packet"]["evidence"])
            .iter()
            .any(|e| e["kind"] == "session");
        doc.text(
            if has_history {
                "No explicit reason was found in the captured conversation."
            } else {
                "No agent conversation was available for this answer."
            },
            AMBER,
        );
        doc.text(
            "This explanation is an interpretation of the available evidence.",
            MUTED,
        );
        doc.gap();
    } else {
        for judgment in recorded {
            let reference = cited
                .iter()
                .position(|e| e["id"] == judgment["quote_id"])
                .map(|i| format!(" [{}]", i + 1))
                .unwrap_or_default();
            doc.text(
                format!("The agent wrote: “{}”{reference}", s(&judgment["quote"])),
                GREEN,
            );
            doc.gap();
        }
    }
    claim(&mut doc, &result["answer"], &cited);
    for judgment in judgments.iter().filter(|j| j["status"] != "recorded") {
        let prefix = if judgment["status"] == "inferred" {
            "Inferred"
        } else {
            "Reason not established"
        };
        doc.text(
            format!(
                "{prefix}: {}{}",
                s(&judgment["reason"]),
                references(judgment, &cited)
            ),
            AMBER,
        );
        doc.gap();
    }
    for key in ["steps", "tradeoffs"] {
        for item in arr(&result[key]) {
            claim(&mut doc, item, &cited);
        }
    }
    for unknown in arr(&result["unknowns"]) {
        let mut text = s(unknown).to_owned();
        for evidence in arr(&artifact["packet"]["evidence"]) {
            let label = cited
                .iter()
                .position(|e| e["id"] == evidence["id"])
                .map(|i| format!("[{}]", i + 1))
                .unwrap_or_else(|| s(&evidence["file"]).into());
            let id = s(&evidence["id"]);
            if !id.is_empty() {
                text = text.replace(id, &label);
            }
        }
        doc.text(text, AMBER);
        doc.gap();
    }
    doc.heading("Sources · click or press s");
    for (i, evidence) in cited.iter().enumerate() {
        let label = if evidence["kind"] == "session" {
            format!(
                "{} · {} conversation",
                s(&evidence["agent"]),
                s(&evidence["role"])
            )
        } else {
            format!("{}:{}", s(&evidence["file"]), n(&evidence["start_line"]))
        };
        doc.sources.push((doc.lines.len(), Link::Source(i)));
        doc.text(format!("[{}] {label}", i + 1), ACCENT);
    }
    if cited.len() > 9 {
        doc.text("/evidence NUMBER opens any reference.", MUTED);
    }
    if !arr(&artifact["stale_files"]).is_empty() || artifact["stale_head"] == true {
        doc.gap();
        doc.text(
            "Code changed while this answer was generated. R updates the answer.",
            AMBER,
        );
    }
    let omitted = n(&artifact["packet"]["omitted_items"]);
    let truncated = arr(&artifact["packet"]["evidence"])
        .iter()
        .filter(|e| e["truncated"] == true)
        .count();
    if omitted > 0 || truncated > 0 {
        doc.text(
            format!("Captured context: {omitted} excerpts omitted, {truncated} shortened."),
            MUTED,
        );
    }
    doc.artifact = Some(artifact);
    doc
}
fn references(claim: &Value, cited: &[Value]) -> String {
    arr(&claim["evidence_ids"])
        .iter()
        .filter_map(|id| cited.iter().position(|e| e["id"] == *id))
        .map(|i| format!(" [{}]", i + 1))
        .collect()
}
fn claim(doc: &mut Document, claim: &Value, cited: &[Value]) {
    if s(&claim["text"]).is_empty() {
        return;
    }
    doc.text(
        format!("{}{}", s(&claim["text"]), references(claim, cited)),
        TEXT,
    );
    doc.gap();
}
pub(super) fn evidence(artifact: Arc<Value>, index: usize) -> Option<Document> {
    let cited = citations(&artifact);
    let e = cited.get(index)?;
    if e["kind"]=="session_edit" {
        let mut doc=Document::new(View::Evidence,s(&e["file"]));
        doc.heading("As implemented · captured edit");
        if e["state"]!="applied" {doc.text("Recorded input · execution unconfirmed",AMBER);}
        super::session_views::captured_code(&mut doc,e);
        if e["truncated"]==true {doc.text("Partial capture",AMBER);}
        doc.gap();
        doc.sources.push((doc.lines.len(),Link::CompareSessionEdit(e["edit_ref"].clone())));
        doc.text("Compare with current code",ACCENT);
        doc.artifact=Some(artifact);return Some(doc);
    }
    let mut doc = Document::new(View::Evidence, format!("[{}] {}", index + 1, s(&e["file"])));
    doc.heading(format!("CAPTURED {}", s(&e["kind"]).to_uppercase()));
    if e["kind"] == "session" {
        provenance(&mut doc, e, true);
    }
    doc.text(
        format!(
            "{}:{} · {}",
            s(&e["file"]),
            n(&e["start_line"]),
            s(&e["id"])
        ),
        MUTED,
    );
    doc.text(
        "Saved with this explanation. This excerpt may differ from current source.",
        MUTED,
    );
    doc.gap();
    let text = e["text"]
        .as_str()
        .or_else(|| e["excerpt"].as_str())
        .unwrap_or("No excerpt captured.");
    if e["kind"] == "code" {
        for (i, line) in text.lines().enumerate() {
            doc.text(
                format!("{:>5}  {line}", n(&e["start_line"]).max(1) + i),
                TEXT,
            );
        }
    } else if e["kind"] == "session" {
        conversation(&mut doc,e,s(&e["agent"]),16000);
    } else {
        doc.text(text, TEXT);
    }
    doc.target = if ["code", "diff"].contains(&s(&e["kind"])) {
        Some(Target {
            file: s(&e["file"]).into(),
            symbol: e["symbol"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            line: n(&e["start_line"]).max(1),
        })
    } else {
        artifact_target(&artifact)
    };
    if e["truncated"] == true {
        doc.gap();
        doc.text(
            "This captured excerpt was truncated; surrounding context may be missing.",
            AMBER,
        );
    }
    doc.artifact = Some(artifact);
    Some(doc)
}
fn provenance(doc: &mut Document, evidence: &Value, links: bool) {
    // Verification notes live inside event details, not repeated above the content.
    if links {
        for reference in arr(&evidence["originals"]) {
            let index = doc.originals.len();
            doc.originals.push(reference.clone());
            doc.sources.push((doc.lines.len(), Link::Source(index)));
            let relation = if reference["relation"] == "retained_context" {
                "retained original context"
            } else {
                "original turn"
            };
            doc.text(
                format!(
                    "[{}] Open {relation} · {} · {}",
                    index + 1,
                    s(&reference["session_id"]),
                    s(&reference["turn_id"])
                ),
                ACCENT,
            );
        }
    }
}
pub(super) fn original(evidence: &Value) -> Document {
    let mut doc = Document::new(View::Original, "Captured original message");
    doc.notice = Some(("Original source · Esc returns to the summary".into(), MUTED));
    provenance(&mut doc, evidence, false);

    doc.text(
        format!("{}:{}", s(&evidence["file"]), n(&evidence["start_line"])),
        MUTED,
    );
    doc.gap();
    conversation(&mut doc,evidence,s(&evidence["agent"]),16000);
    if evidence["truncated"] == true {
        doc.text(
            "Original excerpt shortened by the capture limit; surrounding context may be missing.",
            AMBER,
        );
    }
    doc
}
pub(super) fn commits(entries: &[Value]) -> Document {
    let mut doc = Document::new(View::Commits, "Recent commits");
    doc.notice = Some((
        "↑/↓ select · Enter opens saved conversations · /commit HASH · Esc back".into(),
        MUTED,
    ));
    if entries.is_empty() {
        doc.text(
            "No commits yet. Review your changes before making the first commit.",
            MUTED,
        );
    }
    for (index, entry) in entries.iter().enumerate() {
        doc.sources.push((doc.lines.len(), Link::Source(index)));
        doc.commits.push(s(&entry["commit"]).into());
        doc.text(
            format!("{}  {}", s(&entry["short"]), s(&entry["subject"])),
            TEXT,
        );
    }
    if !entries.is_empty() {
        doc.source_selection = Some(0);
    }
    doc
}

pub(super) fn commit_context(context: &Value) -> Document {
    let hash = s(&context["commit"]);
    let mut doc = Document::new(
        View::Commit,
        format!("Commit {}", hash.chars().take(12).collect::<String>()),
    );
    doc.notice = Some((
        "Saved conversations · local lookup · Esc back · g browse commits".into(),
        MUTED,
    ));
    doc.text(hash, MUTED);
    for link in arr(&context["links"]) {
        let basis = if link["association"] == "snapshot-match" {
            "Matched review base and source snapshot"
        } else {
            "Explicitly linked review"
        };
        doc.text(format!("{basis} · {}", s(&link["review_id"])), ACCENT);
    }
    for (index, session) in arr(&context["sessions"]).iter().enumerate() {
        doc.heading(format!(
            "Capture {} · {}:{}",
            index + 1,
            s(&session["agent"]),
            s(&session["id"])
        ));
        doc.text(
            format!(
                "Saved with {}",
                arr(&session["review_ids"])
                    .iter()
                    .map(s)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            MUTED,
        );
        for event in arr(&session["events"]) {
            provenance(&mut doc, event, true);
            doc.text(
                format!("{}:{}", s(&session["path"]), n(&event["source_line"])),
                MUTED,
            );
            conversation(&mut doc,event,s(&session["agent"]),16000);
        }
        if arr(&session["events"]).is_empty() {
            doc.text("No observable events in this saved capture.", MUTED);
        }
    }
    doc
}

pub(super) fn help() -> Document {
    let mut doc = Document::new(View::Help, "Decisions, evidence and original context");
    for (title, body) in [
        (
            "Start with decisions",
            "d  decisions for the selected captured session
The latest dated captured session is selected initially, not assumed active.
↑/↓ and Enter  open a decision; read why, alternatives and trade-offs
Code evidence comes next; original agent context is the final audit trail.
Recorded / inferred / unknown rationale stays visible throughout.
e  on the overview: explicitly request AI discovery (sends selected evidence)
Opening wy or switching views never calls a model. r refreshes offline.
Missing session edits stay missing; commits and today's edits do not replace them.",
        ),
        (
            "Session work",
            "t  open the selected session's requests and recorded edit sequence
b or /sessions  choose a different captured session
Open an edit to see As implemented; compare with current code on demand.
Partial captures cannot establish a complete historical file.
Original conversation stays behind its own link.
/changes  today's changes, with session ownership left unknown",
        ),
        (
            "Conversation labels",
            "YOUR REQUEST — the user's instruction saved in this session
AGENT MESSAGE — the coding agent's recorded response
AGENT NOTES — saved working notes; expand to read
SESSION SUMMARY — condensed history, not original speech
TOOL ACTION / TOOL RESULT — recorded operations and outputs
UNVERIFIED CONTEXT — source cannot be confirmed
All message bodies use normal text contrast. Dates describe the source event.
An Enriched answer is a NEW wy assessment, not the original conversation",
        ),
        (
            "AI assessments",
            "e / R  request decisions using only the selected session's captured edits
No current code is sent as historical evidence. Requests can consume CLI usage.
1–9  open a decision or numbered source · s  select links
Keep browsing while it runs; a result never changes your selected session.
Esc  back · x  cancel running and queued requests
/why and /reason are separate, opt-in current-code explanation tools",
        ),
        (
            "Navigate",
            "Ctrl+←/→  cycle Decisions / Sessions · d / t  jump directly
Tab  session work · b  session picker
Alt+←/→  previous / next page · z  collapse / expand code
↑/↓ or j/k  select links or scroll · ←/→ or h/l  pan code
PageUp/PageDown  scroll · Home/End  start/end
r  refresh captured sessions; preserve the selected session when available
g  browse commits · /commit HASH  read saved conversations
q / Ctrl+Q / Ctrl+C  quit",
        ),
        (
            "Settings & commands",
            "/agent codex|claude
/source all|codex|claude|pi|opencode|none (both = all)
/coverage — capture counts, exclusions, and linkage gaps
/sessions — browse dated sessions across tools
/timeline [FILE:SYMBOL] — chronological captured turns
/decisions — selected-session decisions
/changes — unassigned working-tree changes
/decisions discover — explicitly request a prioritized AI brief
/decisions FILE:SYMBOL — recorded file context, tests, and gaps
/setup then /setup save — optional decision-record template
/export then /export save — export the working-tree review (not the session brief)
/why FILE:SYMBOL QUESTION
/ask QUESTION
/reason QUESTION — question about all changes
/evidence NUMBER
/commits
/commit HASH
/link HASH [REVIEW-ID] — explicitly attach a saved review
/cancel",
        ),
        (
            "About the evidence",
            "Reasons, changes and sources are local: no model call. A reason is what the agent wrote, matched to the edit it recorded; it is not hidden reasoning and does not prove who typed the final text. Enrich uses your selected agent and may use your account's allowance.",
        ),
    ] {
        doc.heading(title);
        doc.text(body, TEXT);
    }
    doc
}
