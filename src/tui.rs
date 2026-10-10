//! Read-only review workspace. Agent requests run only after explicit input.
mod document;
mod decision_views;
mod session_views;
mod session_flow;
#[cfg(test)]
mod decision_tests;
mod code_view;
mod history_views;
mod explorer;
mod layout;
mod navigation;
mod render;
use crate::{agent::Cancel, arr, n, presentation, reasoning, s, security, service};
use anyhow::{Result, ensure};
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use document::{Document, Link, View};
use explorer::{Explorer, Kind, Target};
use layout::{Divider, PaneSizes};
use navigation::{ReadingState, artifact_key, options_key};
use ratatui::{
    prelude::*,
    widgets::{
        List, ListItem, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
};
use serde_json::Value;
use std::{
    collections::VecDeque,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

/// The terminal's own palette: wy adds no backgrounds, so it follows the user's theme
/// and works on light terminals. Hierarchy comes from labels, spacing and weight;
/// meaningful content never depends on low-contrast terminal dark gray.
mod theme {
    use ratatui::style::Color;
    pub const TEXT: Color = Color::Reset;
    pub const MUTED: Color = Color::Reset;
    pub const BORDER: Color = Color::DarkGray;
    pub const ACCENT: Color = Color::Cyan;
    pub const GREEN: Color = Color::Green;
    pub const RED: Color = Color::Red;
    pub const AMBER: Color = Color::Yellow;
}
use theme::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Files,
    Reader,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Input {
    Command,
    Question,
    Filter,
}
enum Update {
    Progress(String),
    Done(Result<Value>),
}
struct Job {
    receiver: mpsc::Receiver<Update>,
    cancel: Cancel,
    handle: thread::JoinHandle<()>,
    started: Instant,
    scope: String,
    key: String,
    file: Option<String>,
    progress: String,
}
/// A draft keeps this context even if a background answer arrives while typing.
#[derive(Clone, Default)]
struct QuestionContext {
    file: Option<String>,
    target: Option<String>,
    previous: Option<Arc<Value>>,
    session_edit: Option<Value>,
    note_refs: Vec<Value>,
}
impl QuestionContext {
    fn label(&self) -> &str {
        self.target
            .as_deref()
            .or(self.file.as_deref())
            .unwrap_or("all changes")
    }
    fn options(&self, question: &str, agent: &str, source: &str) -> reasoning::Options {
        let question = if let Some(artifact) = &self.previous {
            format!(
                "Previous question: {}\nPrevious assessment:\n{}\n\nFollow-up: {question}",
                s(&artifact["packet"]["question"]),
                presentation::reasoning_text(artifact)
            )
        } else {
            question.into()
        };
        reasoning::Options {
            file: self.file.clone(),
            target: self.target.clone(),
            question,
            agent: agent.into(),
            source: source.into(),
            session_edit: self.session_edit.clone(),
            note_refs: self.note_refs.clone(),
        }
    }
}
#[derive(Default)]
struct Areas {
    body: Rect,
    file_divider: Rect,
    files: Rect,
    reader: Rect,
    input: Rect,
    tabs: Vec<(Rect, View, Focus)>,
    sources: Vec<(Rect, Link)>,
}
struct Workspace {
    root: PathBuf,
    review: Value,
    explorer: Explorer,
    document: Document,
    back: Vec<(Document, Focus, Option<Document>)>,
    code: Option<Document>,
    answers: Vec<Arc<Value>>,
    decision_brief: Option<Arc<Value>>,
    selected_work: Option<Value>,
    sessions: Vec<Value>,
    history_tabs: std::collections::HashMap<String,Document>,
    views: Vec<ReadingState>,
    queue: VecDeque<reasoning::Options>,
    focus: Focus,
    agent: String,
    source: String,
    input: String,
    editing: Option<Input>,
    draft: Option<QuestionContext>,
    sidebar: bool,
    /// Collapse every reason to its headline (w toggles).
    brief: bool,
    pane_sizes: PaneSizes,
    dragging: Option<Divider>,
    status: String,
    error: bool,
    job: Option<Job>,
    areas: Areas,
    scroll_max: u16,
    page_size: u16,
}
impl Workspace {
    fn new(root: &Path) -> Result<Self> {
        let review = service::review(
            root,
            &service::ReviewOptions {
                source: "both".into(),
                ..Default::default()
            },
        )?;
        let sessions = crate::history::saved(&review)?;
        let mut app = Self::from_review(root, review);
        app.pane_sizes = PaneSizes::load(root)?;
        app.sessions = sessions;
        for answer in crate::storage::Store::open(root)?
            .recent("reasoning", 40)?
            .into_iter()
            .rev()
        {
            if answer["packet"].is_object() && answer["explanation"].is_object() {
                app.cache_answer(Arc::new(answer));
            }
        }
        app.views.clear();
        app.document = document::empty(&app.review);
        app.code = None;
        app.preview_selection();
        app.select_latest_work();
        app.decision_brief = crate::decisions::saved(root, &app.decision_scope())?.map(Arc::new);
        // The secondary current-file browser must not mix in earlier sessions.
        app.review["recent_code"]=serde_json::json!([]);app.explorer.rebuild(&app.review);
        app.decision_home();
        app.back.clear();
        Ok(app)
    }
    fn from_review(root: &Path, review: Value) -> Self {
        let mut explorer = Explorer::default();
        explorer.rebuild(&review);
        if let Some(index) = explorer.rows.iter().position(|r| r.kind == Kind::File) {
            explorer.state.select(Some(index));
        }
        let document = explorer
            .target()
            .map(|target| document::preview(&review, target))
            .unwrap_or_else(|| document::empty(&review));
        let mut app = Self {
            root: root.into(),
            review,
            explorer,
            document,
            back: vec![],
            code: None,
            answers: vec![],
            decision_brief: None,
            selected_work: None,
            sessions: vec![],
            history_tabs: std::collections::HashMap::new(),
            views: vec![],
            queue: VecDeque::new(),
            focus: Focus::Files,
            agent: "codex".into(),
            source: "both".into(),
            input: String::new(),
            editing: None,
            draft: None,
            sidebar: true,
            brief: false,
            pane_sizes: PaneSizes::default(),
            dragging: None,
            status: String::new(),
            error: false,
            job: None,
            areas: Areas::default(),
            scroll_max: 0,
            page_size: 12,
        };
        app.preview_selection();
        app
    }
    /// Primary section, retained when opening a decision's cited evidence.
    fn section(&self) -> View {
        if matches!(self.document.kind,View::DecisionOverview|View::DecisionDetail) || self.document.artifact.as_ref().is_some_and(|a|a["context"]=="decision_brief") {
            View::DecisionOverview
        } else if self.document.historical() {
            View::Sessions
        } else {View::Recorded}
    }
    fn session_context(&self)->bool {
        self.document.artifact.as_ref().is_some_and(|a|a["scope_kind"]=="session" || a["context"]=="session_work")
    }
    fn decision_scope(&self)->Value {
        self.selected_work.clone().unwrap_or_else(||crate::session_work::empty(&self.review))
    }
    fn select_latest_work(&mut self) {
        let previous=self.selected_work.as_ref();
        let selected=previous.and_then(|w|self.sessions.iter().find(|s|s["id"]==w["session"]["id"] && s["agent"]==w["session"]["agent"]))
            .or_else(||crate::session_work::latest(&self.sessions));
        self.selected_work=selected.map(|s|crate::session_work::build(&self.review,s));
    }
    fn session_flow(&mut self) {
        self.remember_section();
        if self.selected_work.is_none(){self.select_latest_work();}
        let work=Arc::new(self.decision_scope());
        let key=format!("session-flow:{}",s(&work["session"]["storage_key"]));
        let doc=self.history_tabs.get(&key).cloned().unwrap_or_else(||session_views::flow(work,0));
        self.open(doc);self.sidebar=false;self.code=None;
    }
    fn session_picker(&mut self) {
        self.open(session_views::picker(&self.review,&self.sessions,self.selected_work.as_ref()));self.sidebar=false;
    }
    fn select_session(&mut self,key:&str)->Result<()> {
        let session=crate::storage::Store::open(&self.root)?.get("session",key)?;
        crate::validate("Session",&session)?;
        ensure!(crate::history::belongs(s(&session["cwd"]),&self.root) && crate::session_work::snapshot_key(&session)==key,"Session snapshot is not verifiably scoped to this repository");
        let work=crate::session_work::build(&self.review,&session);
        self.decision_brief=crate::decisions::saved(&self.root,&work)?.map(Arc::new);
        self.selected_work=Some(work);self.views.clear();self.session_flow();self.back.clear();Ok(())
    }
    fn remember_section(&mut self) {
        if self.document.kind==View::SessionWork {
            let key=format!("session-flow:{}",self.document.artifact.as_ref().map(|a|s(&a["session"]["storage_key"])).unwrap_or(""));
            if self.history_tabs.len()>=20{self.history_tabs.clear();}
            self.history_tabs.insert(key,self.document.clone());
        }
    }
    fn decision_home(&mut self) {
        self.remember_section();
        if self.selected_work.is_none(){self.select_latest_work();}
        let scope=self.decision_scope();
        let sessions:Vec<_>=self.sessions.iter().filter(|session|crate::session_work::snapshot_key(session)==s(&scope["session"]["storage_key"])).cloned().collect();
        let sessions=if sessions.is_empty(){crate::session_work::load(&self.root,&scope).ok().into_iter().collect()}else{sessions};
        let brief = self.decision_brief.clone().filter(|a|a["scope_key"]==crate::decisions::scope_key(&scope))
            .unwrap_or_else(||Arc::new(crate::decisions::recorded(&scope,&sessions)));
        self.open(decision_views::overview(brief));
        self.sidebar=false;
        self.code=None;
    }
    fn discover_decisions(&mut self) {
        if self.job.is_some() {
            self.message("Wait for the current request or cancel it with x before discovering decisions");
            return;
        }
        if self.review["history_source"]!=self.source {
            if let Err(error)=self.refresh() {self.fail(error);return;}
        }
        let scope=self.decision_scope();
        if arr(&scope["edits"]).is_empty() {
            self.message("No captured code edits in the selected session · current files will not be substituted");
            return;
        }
        let root=self.root.clone();
        let review=scope;
        let agent=self.agent.clone();
        let cancel=Arc::new(AtomicBool::new(false));
        let worker_cancel=cancel.clone();
        let (sender,receiver)=mpsc::channel();
        let handle=thread::spawn(move || {
            let result=crate::decisions::run(&root,&review,&agent,&worker_cancel,|p|{let _=sender.send(Update::Progress(p.into()));});
            let _=sender.send(Update::Done(result));
        });
        self.job=Some(Job{receiver,cancel,handle,started:Instant::now(),scope:"session decisions".into(),key:"decision-brief".into(),file:None,progress:"Preparing decision discovery…".into()});
        self.message("Decision discovery requested · selected evidence is sent to your reasoning CLI · keep browsing");
    }
    fn finish_decisions(&mut self, artifact: Arc<Value>) {
        if artifact["scope_key"]!=crate::decisions::scope_key(&self.decision_scope()) {
            self.message("Decision brief saved for an earlier capture · refresh and request a new brief");
            return;
        }
        self.decision_brief=Some(artifact.clone());
        // Do not interrupt a deep dive, scrolled reader, evidence view or draft.
        if self.document.kind==View::DecisionOverview && self.document.scroll==0 && self.editing.is_none() {
            self.document=decision_views::overview(artifact);
            let mut doc=self.document.clone();
            self.check_freshness(&mut doc);
            self.document=doc;
        }
        self.message("Decisions ready");
    }
    fn message(&mut self, message: impl Into<String>) {
        self.status = message.into();
        self.error = false;
    }
    fn fail(&mut self, error: impl std::fmt::Display) {
        self.status = security::redact(&error.to_string());
        self.error = true;
    }
    fn open(&mut self, mut document: Document) {
        self.remember_view();
        self.check_freshness(&mut document);
        if document.kind == View::Explanation {
            if let Some(artifact) = &document.artifact {
                self.cache_answer(artifact.clone());
            }
        }
        let code = self.code_for_answer(&document);
        let old = std::mem::replace(&mut self.document, document);
        let old_code = std::mem::replace(&mut self.code, code);
        self.back.push((old, self.focus, old_code));
        if self.back.len() > 40 {
            self.back.remove(0);
        }
        self.focus = Focus::Reader;
        self.remember_view();
    }
    fn code_for_answer(&self, document: &Document) -> Option<Document> {
        let artifact = document.artifact.as_ref()?;
        let target = document::artifact_target(artifact)?;
        let recorded = &artifact["packet"]["focus_session_edit"];
        for code in std::iter::once(&self.document).chain(self.code.as_ref()) {
            if code.code()
                && code
                    .target
                    .as_ref()
                    .is_some_and(|t| t.selector() == target.selector())
                && code
                    .session_edit
                    .as_ref()
                    .map(|e| &e["id"])
                    .unwrap_or(&Value::Null)
                    == &recorded["id"]
            {
                return Some(code.clone());
            }
        }
        if recorded.is_object() {
            if let Some(edit) = document::recent_edit(&self.review, &target.file)
                .filter(|e| e["id"] == recorded["id"])
            {
                return Some(document::recorded_code(edit));
            }
            return crate::history::saved_edit(&self.root, recorded)
                .ok()
                .map(|(edit, _)| document::recorded_code(&edit));
        }
        Some(document::diff(&self.review, target))
    }
    fn check_freshness(&self, document: &mut Document) {
        let Some(artifact) = &document.artifact else {
            return;
        };
        if document.kind==View::Export {
            document.notice=Some(("Export of a dated snapshot, not a live review · preview sensitive content before saving".into(),AMBER));
            return;
        }
        if document::is_recorded(artifact) || artifact["scope_kind"]=="session" || artifact["context"]=="session_work" {
            return;
        }
        let Some(review_id) = artifact["review_id"].as_str() else {
            return;
        };
        let current = (|| -> Result<bool> {
            let saved = crate::storage::Store::open(&self.root)?.get("review", review_id)?;
            let sources = crate::repository::sources(&self.root)?;
            Ok(saved["file_hashes"] == serde_json::json!(sources.hashes)
                && saved["head"] == serde_json::json!(crate::repository::head(&self.root)))
        })();
        document.notice = Some(match current {
            Ok(true) if artifact["context"]=="decision_brief" => {document.notice=None;return;}
            Ok(true) => ("Source matches the captured review".into(), GREEN),
            Ok(false) => (
                "SOURCE CHANGED · this describes an earlier version. Refresh with r before requesting an update."
                    .into(),
                AMBER,
            ),
            Err(_) => (
                "Saved review unavailable · source freshness could not be checked".into(),
                AMBER,
            ),
        });
    }
    fn saved_explanation(&mut self) -> Result<()> {
        let artifact = crate::storage::Store::open(&self.root)?
            .get("reasoning", "latest")
            .map_err(|_| anyhow::anyhow!("No saved answer yet · select a change and press e"))?;
        self.open(document::explanation(Arc::new(artifact)));
        self.message("Saved explanation opened · source freshness checked · 1–9 opens evidence");
        Ok(())
    }
    fn target(&self) -> Option<Target> {
        if self.focus == Focus::Files {
            self.explorer.target()
        } else if self.document.historical() {
            None
        } else {
            self.document
                .target
                .clone()
                .or_else(|| self.explorer.target())
        }
    }
    fn required_target(&mut self) -> Option<Target> {
        let target = self.target();
        if target.is_none() {
            self.message("Select a changed file or function first");
        }
        target
    }
    fn preview_selection(&mut self) {
        let target = self.explorer.target();
        if let Some(target) = target {
            self.show_code(document::preview(&self.review, target), false);
        } else {
            self.remember_view();
            self.document = document::empty(&self.review);
            self.code = None;
        }
    }
    fn why_change(&mut self, refresh: bool) {
        if self.session_context() {
            self.message("Open Decisions to assess this session's captured work");return;
        }
        if self.focus == Focus::Reader && self.document.historical() {
            self.message(
                "Browsing saved commit conversations · select a current file to enrich it",
            );
            return;
        }
        let existing = (self.focus == Focus::Reader)
            .then(|| {
                self.document
                    .artifact
                    .clone()
                    .filter(|a| !document::is_recorded(a))
            })
            .flatten();
        if !refresh {
            if let Some(artifact) = existing.clone() {
                if self.document.kind != View::Explanation {
                    self.open(document::explanation(artifact));
                }
                return;
            }
        }
        let Some(options) = self.why_options(refresh) else {
            self.message("Select a changed file or function first");
            return;
        };
        if !refresh {
            if let Some(answer) = self
                .answers
                .iter()
                .rev()
                .find(|a| {
                    document::artifact_target(a).map(|t| t.selector())
                        == options.target.clone().or(options.file.clone())
                        && &a["packet"]["focus_session_edit"]["id"]
                            == options
                                .session_edit
                                .as_ref()
                                .map(|e| &e["id"])
                                .unwrap_or(&Value::Null)
                })
                .cloned()
            {
                self.open(document::explanation(answer));
                self.message("Saved answer · R updates it");
                return;
            }
        }
        self.start(options);
    }
    fn why_options(&self, _refresh: bool) -> Option<reasoning::Options> {
        if self.session_context(){return None;}
        let target = if self.focus == Focus::Reader {
            if let Some(artifact) = &self.document.artifact {
                document::artifact_target(artifact)
            } else {
                Some(self.target()?)
            }
        } else {
            Some(self.target()?)
        };
        let session_edit = if self.focus == Focus::Reader {
            self.document.session_edit.clone().or_else(|| {
                {
                    self.document.artifact.as_ref().and_then(|a| {
                        a["packet"]["focus_session_edit"]
                            .as_object()
                            .map(|_| a["packet"]["focus_session_edit"].clone())
                    })
                }
            })
        } else {
            target
                .as_ref()
                .filter(|t| {
                    !arr(&self.review["changes"])
                        .iter()
                        .any(|c| c["file"] == t.file)
                })
                .and_then(|t| document::recent_edit(&self.review, &t.file))
                .map(crate::history::edit_ref)
        };
        let mut options = self.options(
            target,
            true,
            Some(reasoning::prompt("enrich_change").into()),
        );
        if let Some(edit) = session_edit {
            options.target = None;
            options.file = edit["file"].as_str().map(str::to_owned);
            options.session_edit = Some(edit);
        }
        if let Some(artifact) = &self.document.artifact {
            if document::artifact_target(artifact).map(|t| t.selector())
                == options.target.clone().or(options.file.clone())
            {
                options.note_refs = arr(&artifact["packet"]["note_refs"]).to_vec();
                let gaps = arr(&artifact["packet"]["gaps"])
                    .iter()
                    .map(s)
                    .collect::<Vec<_>>()
                    .join(" ");
                if !gaps.is_empty() {
                    options.question.push_str(&format!("\nNotes about the captured excerpts: {gaps}"));
                }
            }
        }
        Some(options)
    }
    fn options(
        &self,
        target: Option<Target>,
        design: bool,
        question: Option<String>,
    ) -> reasoning::Options {
        reasoning::Options {
            agent: self.agent.clone(),
            source: self.source.clone(),
            question: question.unwrap_or_else(|| {
                reasoning::prompt(if design { "why" } else { "explain" }).into()
            }),
            target: target
                .as_ref()
                .filter(|t| design || t.symbol.is_some())
                .map(Target::selector),
            file: target.map(|t| t.file),
            session_edit: None,
            note_refs: vec![],
        }
    }
    fn start(&mut self, options: reasoning::Options) {
        let key = options_key(&options);
        if self.job.as_ref().is_some_and(|j| j.key == key)
            || self.queue.iter().any(|q| options_key(q) == key)
        {
            self.message("This enrichment is already running or queued · keep browsing");
            return;
        }
        if self.job.is_some() {
            if self.queue.len() >= 8 {
                self.message("Eight enrichments are queued · wait for one to finish");
                return;
            }
            let file = options.file.clone().unwrap_or_else(|| "all changes".into());
            self.queue.push_back(options);
            self.message(format!("Queued {file} · you can keep browsing"));
            return;
        }
        self.launch(options);
    }
    fn launch(&mut self, options: reasoning::Options) {
        let scope = options
            .target
            .clone()
            .or(options.file.clone())
            .unwrap_or_else(|| "all changes".into());
        let key = options_key(&options);
        let file = options.file.clone();
        let root = self.root.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (sender, receiver) = mpsc::channel();
        let handle = thread::spawn(move || {
            let result = reasoning::run(&root, &options, &worker_cancel, |p| {
                let _ = sender.send(Update::Progress(p.into()));
            });
            let _ = sender.send(Update::Done(result));
        });
        self.job = Some(Job {
            receiver,
            cancel,
            handle,
            started: Instant::now(),
            scope,
            key,
            file,
            progress: "Preparing enrichment…".into(),
        });
        self.message("Enrichment runs in the background · keep browsing");
    }
    fn poll(&mut self) {
        let mut completed = None;
        while let Some(job) = self.job.as_mut() {
            match job.receiver.try_recv() {
                Ok(Update::Progress(progress)) => job.progress = progress,
                Ok(Update::Done(result)) => {
                    completed = Some(result);
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    completed = Some(Err(anyhow::anyhow!(
                        "Enrichment worker stopped unexpectedly; try again"
                    )));
                    break;
                }
            }
        }
        if let Some(result) = completed {
            let job = self.job.take().unwrap();
            let _ = job.handle.join();
            match result {
                Ok(artifact) if artifact["context"]=="decision_brief" => self.finish_decisions(Arc::new(artifact)),
                Ok(artifact) => self.finish_answer(&job.key, Arc::new(artifact)),
                Err(error) => self.fail(format!("{}: {error:#}", job.scope)),
            }
            if let Some(next) = self.queue.pop_front() {
                self.launch(next);
            }
        }
    }
    fn finish_answer(&mut self, request_key: &str, artifact: Arc<Value>) {
        let key = artifact_key(&artifact);
        self.cache_answer(artifact.clone());
        let mut doc = document::explanation(artifact);
        self.check_freshness(&mut doc);
        if let Some(saved) = self
            .views
            .iter_mut()
            .find(|v| v.key == key || v.key == request_key)
        {
            if saved.document.kind != View::Evidence {
                saved.document = doc.clone();
            }
        }
        // Never move the user to another file, close a source, or interrupt a draft.
        if self.current_key().as_deref() == Some(request_key)
            && self.editing.is_none()
            && self.document.kind == View::Recorded
            && self.document.scroll == 0
        {
            self.document = doc.clone();
            self.remember_view();
        }
        self.message(format!(
            "Enrichment ready · {} · v opens it",
            doc.target
                .as_ref()
                .map(Target::selector)
                .unwrap_or_else(|| "all changes".into())
        ));
    }
    fn cancel(&mut self) {
        self.queue.clear();
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Relaxed);
            self.message("Cancelling request…");
        }
    }
    fn question_context(&self) -> QuestionContext {
        if let Some(draft) = &self.draft {
            return draft.clone();
        }
        if self.focus == Focus::Reader {
            if let Some(artifact) = &self.document.artifact {
                return QuestionContext {
                    file: document::artifact_target(artifact).map(|t| t.file),
                    target: artifact["packet"]["focus_target"]["target"]
                        .as_str()
                        .map(str::to_owned),
                    previous: (!document::is_recorded(artifact) && artifact["context"]!="decision_brief" && artifact["context"]!="session_work").then(|| artifact.clone()),
                    note_refs: arr(&artifact["packet"]["note_refs"]).to_vec(),
                    session_edit: artifact["packet"]["focus_session_edit"]
                        .as_object()
                        .map(|_| artifact["packet"]["focus_session_edit"].clone()),
                };
            }
            if self.document.target.is_none() {
                return QuestionContext::default();
            }
        }
        let target = self.target();
        QuestionContext {
            file: target.as_ref().map(|t| t.file.clone()),
            target: target
                .as_ref()
                .filter(|t| t.symbol.is_some())
                .map(Target::selector),
            previous: None,
            note_refs: vec![],
            session_edit: self.document.session_edit.clone(),
        }
    }
    fn question_options(&self, question: &str) -> reasoning::Options {
        self.question_context()
            .options(question, &self.agent, &self.source)
    }
    fn question_scope(&self) -> String {
        self.question_context().label().into()
    }
    fn command(&mut self) -> Result<()> {
        let input = self.input.trim().to_owned();
        let question_options = self.question_options(&input);
        let mode = self.editing.take();
        let context = self.question_context();
        self.draft = None;
        self.input.clear();
        if mode == Some(Input::Filter) {
            self.message("Filter applied · f edits it · Esc clears it in files");
            return Ok(());
        }
        if input.is_empty() {
            return Ok(());
        }
        if mode == Some(Input::Question) && !input.starts_with('/') {
            ensure!(!self.session_context(),"Open Decisions to assess captured session evidence; current-code follow-ups are separate");
            self.start(question_options);
            return Ok(());
        }
        let (name, rest) = input.split_once(' ').unwrap_or((&input, ""));
        let rest = rest.trim();
        match name {
            "/coverage" => self.open(history_views::coverage(&self.review,&self.sessions)),
            "/sessions" => self.session_picker(),
            "/changes" => self.open(session_views::working_changes(&self.review)),
            "/decisions" if rest.is_empty() => self.decision_home(),
            "/decisions" if rest=="discover" => self.discover_decisions(),
            "/timeline" | "/decisions" => {
                let target=if rest.is_empty(){self.document.target.clone().or_else(||self.explorer.target()).ok_or_else(||anyhow::anyhow!("Select a file or use /timeline FILE[:SYMBOL]"))?}else{
                    let (file,symbol)=rest.split_once(':').map(|(f,s)|(f,Some(s.to_owned()))).unwrap_or((rest,None));
                    ensure!(security::allowed(file),"Choose an eligible repository-relative file");
                    Target{file:file.into(),symbol,line:1}
                };
                self.open(history_views::context(&self.review,&self.sessions,target,name=="/decisions"));
            }
            "/setup" => {
                if rest=="save"{
                    ensure!(self.document.kind==View::Setup,"Preview /setup before saving the optional instructions");
                    let path=crate::insights::save_private(&self.root,"decision-instructions.md",crate::insights::DECISION_INSTRUCTIONS)?;
                    self.message(format!("Saved {} · agent configuration was NOT modified",path.display()));
                }else{ensure!(rest.is_empty(),"Use /setup, then /setup save");self.open(history_views::setup());}
            }
            "/export" => {
                if rest=="save"{
                    ensure!(self.document.kind==View::Export,"Preview /export before saving");
                    let artifact=self.document.artifact.as_ref().ok_or_else(||anyhow::anyhow!("Export preview unavailable"))?;
                    ensure!(artifact["review_id"]==self.review["id"],"Capture changed; preview /export again");
                    let name=format!("review-brief-{}.md",&security::digest(s(&artifact["export_text"]))[..16]);
                    let path=crate::insights::save_private(&self.root,&name,s(&artifact["export_text"]))?;
                    self.message(format!("Saved {} · nothing uploaded; review before sharing",path.display()));
                }else{ensure!(rest.is_empty(),"Use /export, then /export save");self.open(history_views::export(&self.review,&self.sessions));}
            }
            "/commits" => self.open_commits()?,
            "/commit" => {
                ensure!(!rest.is_empty(), "Use /commit HASH or /commits to browse");
                self.open_commit(rest)?;
            }
            "/link" => {
                let fields: Vec<_> = rest.split_whitespace().collect();
                ensure!(
                    (1..=2).contains(&fields.len()),
                    "Use /link HASH [REVIEW-ID]"
                );
                let linked = crate::commits::link(&self.root, fields[0], fields.get(1).copied())?;
                self.open_commit(s(&linked["commit"]))?;
                self.message(format!(
                    "Linked saved review {} to commit {}",
                    s(&linked["review_id"]),
                    s(&linked["commit"])
                ));
            }
            "/layout" => {
                ensure!(rest == "reset", "Use /layout reset to restore pane sizes");
                self.pane_sizes = PaneSizes::default();
                self.save_layout()?;
                self.message("Default file tree width restored");
            }
            "/reason" => {
                self.start(self.options(None, false, (!rest.is_empty()).then(|| rest.into())))
            }
            "/why" => {
                let (target, question) = rest
                    .split_once(' ')
                    .unwrap_or((rest, reasoning::prompt("why")));
                ensure!(!target.is_empty(), "Use /why FILE:SYMBOL QUESTION");
                let resolved = reasoning::resolve_target(&self.root, target)?;
                let selected = Target {
                    file: s(&resolved["file"]).into(),
                    symbol: Some(s(&resolved["symbol"]).into()),
                    line: n(&resolved["start_line"]),
                };
                let mut options = self.options(Some(selected), true, Some(question.into()));
                options.target = Some(target.into());
                self.start(options);
            }
            "/agent" => {
                ensure!(
                    ["codex", "claude"].contains(&rest),
                    "Use /agent codex or /agent claude"
                );
                self.agent = rest.into();
                self.message(format!("Next explanation uses {rest}"));
            }
            "/source" => {
                crate::history::valid_source(rest)?;
                self.source = rest.into();
                self.message(format!(
                    "Next request uses {rest} history · r refreshes offline coverage"
                ));
            }
            "/cancel" => self.cancel(),
            "/ask" => {
                ensure!(!rest.is_empty(), "Use /ask QUESTION");
                ensure!(!self.session_context(),"Open Decisions to assess captured session evidence; current-code follow-ups are separate");
                ensure!(
                    !(self.focus == Focus::Reader && self.document.historical()),
                    "Select a current file to ask a question; this view contains saved commit conversations"
                );
                self.start(context.options(rest, &self.agent, &self.source));
            }
            "/evidence" => {
                let index: usize = rest
                    .parse()
                    .map_err(|_| anyhow::anyhow!("Use /evidence NUMBER"))?;
                ensure!(index > 0, "Evidence numbers start at 1");
                self.open_evidence(index - 1)?;
            }
            _ if input.starts_with('/') => self.message("Unknown command · ? opens help"),
            _ => {
                ensure!(!self.session_context(),"Open Decisions to assess captured session evidence; current-code follow-ups are separate");
                ensure!(
                    !(self.focus == Focus::Reader && self.document.historical()),
                    "Select a current file to ask a question; this view contains saved commit conversations"
                );
                self.start(question_options);
            }
        }
        Ok(())
    }
    fn open_evidence(&mut self, index: usize) -> Result<()> {
        if !self.document.originals.is_empty() {
            let reference=self.document.originals.get(index).ok_or_else(||anyhow::anyhow!("No original source at this position"))?;
            let evidence=crate::history::origins::open(&self.root,reference)?;
            self.open(document::original(&evidence));
            return Ok(());
        }
        if self.document.kind == View::Commits {
            let commit = self
                .document
                .commits
                .get(index)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("No commit at this position"))?;
            return self.open_commit(&commit);
        }
        let artifact = self.document.artifact.clone().ok_or_else(|| {
            anyhow::anyhow!("Open an explanation first to inspect its references")
        })?;
        let doc = document::evidence(artifact, index)
            .ok_or_else(|| anyhow::anyhow!("No reference [{}] in this explanation", index + 1))?;
        self.open(doc);
        Ok(())
    }
    fn follow(&mut self, link: Link) -> Result<()> {
        match link {
            Link::DecisionHome => {self.decision_home();Ok(())}
            Link::DiscoverDecisions => {self.discover_decisions();Ok(())}
            Link::Decision(index) => {
                let brief=self.document.artifact.clone().filter(|a|a["context"]=="decision_brief").ok_or_else(||anyhow::anyhow!("Open the decision overview first"))?;
                let doc=decision_views::detail(brief,index).ok_or_else(||anyhow::anyhow!("Decision not found"))?;
                self.open(doc);self.code=None;Ok(())
            }
            Link::Page(index) => {
                if let Some(page)=self.document.pagination.clone(){
                    self.document=history_views::page(&self.review,&page,index);
                    self.focus=Focus::Reader;
                }
                Ok(())
            }
            Link::Explain => {self.why_change(false);Ok(())}
            Link::Disclosure(id) => {
                self.document.toggle_disclosure(&id);
                Ok(())
            }
            Link::Session(key) => self.select_session(&key),
            Link::SessionPicker => {self.session_picker();Ok(())}
            Link::SessionEdit(reference) => {
                let (edit,_)=crate::history::saved_edit(&self.root,&reference)?;
                self.open(session_views::implementation(&edit));Ok(())
            }
            Link::CompareSessionEdit(reference) => {
                let (edit,session)=crate::history::saved_edit(&self.root,&reference)?;
                let work=crate::session_work::build(&self.review,&session);
                self.open(session_views::comparison(&self.root,&work,&edit));Ok(())
            }
            Link::SessionChat(key) => {
                let session=crate::storage::Store::open(&self.root)?.get("session",&key)?;
                crate::validate("Session",&session)?;
                ensure!(crate::history::belongs(s(&session["cwd"]),&self.root),"Session is not scoped to this repository");
                self.open(history_views::session(&session));Ok(())
            }
            Link::Source(index) => self.open_evidence(index),
            Link::Turn(edit) => {
                let (_, session) = crate::history::saved_edit(&self.root, &crate::history::edit_ref(&edit))?;
                self.open(document::turn(&session, &edit));
                Ok(())
            }
            Link::Line(line) => {
                self.document.scroll = line.min(u16::MAX as usize) as u16;
                self.document.source_selection = None;
                Ok(())
            }
        }
    }
    /// Rebuild the reader for the selected file, keeping the reading position.
    fn rebuild_reader(&mut self) {
        let Some(code) = self.code.clone() else { return };
        if self.document.kind != View::Recorded {
            return;
        }
        let (scroll, selection) = (self.document.scroll, self.document.source_selection);
        let disclosures:Vec<_>=self.document.sources.iter().filter_map(|(_,link)|if let Link::Disclosure(id)=link{Some((id.clone(),self.document.expanded.contains(id)))}else{None}).collect();
        self.document = self.local_notes(&code);
        for (id,open) in disclosures {if self.document.expanded.contains(&id)!=open{self.document.toggle_disclosure(&id);}}
        self.document.scroll = scroll;
        self.document.source_selection =
            selection.filter(|&i| i < self.document.sources.len());
    }
    fn open_commits(&mut self) -> Result<()> {
        let entries = crate::commits::recent(&self.root)?;
        self.open(document::commits(&entries));
        self.message("Select a commit and press Enter · /commit HASH opens any revision");
        Ok(())
    }
    fn open_commit(&mut self, revision: &str) -> Result<()> {
        let context = crate::commits::lookup(&self.root, revision, "both").map_err(|error| {
            if error.to_string().starts_with("No saved conversations linked") {
                anyhow::anyhow!("No saved conversation matches this commit · /link {revision} [REVIEW-ID] attaches a saved review")
            } else {
                error
            }
        })?;
        self.open(document::commit_context(&context));
        self.message("Saved commit conversations · Esc goes back · g browses commits");
        Ok(())
    }
    fn edit(&mut self, mode: Input) {
        self.draft = if mode == Input::Filter {
            None
        } else {
            Some(self.question_context())
        };
        self.editing = Some(mode);
        self.input = match mode {
            Input::Command => "/".into(),
            Input::Question => String::new(),
            Input::Filter => self.explorer.filter.clone(),
        };
        if mode == Input::Filter {
            self.focus = Focus::Files;
            self.sidebar = true;
        }
    }
    fn filter_input(&mut self) {
        if self.editing == Some(Input::Filter) {
            self.explorer.filter = self.input.clone();
            self.explorer.rebuild(&self.review);
            self.preview_selection();
        }
    }
    fn change_view(&mut self, view: View) -> Result<()> {
        self.remember_section();
        if view==View::Recorded {self.sidebar=true;}
        if view==View::Sessions {
            if self.document.kind!=View::SessionWork {self.session_flow();}
            self.focus=Focus::Reader;return Ok(());
        }
        if self.document.kind==view {self.focus=Focus::Reader;return Ok(());}
        if self.document.kind==View::Recorded {
            if let Some(code)=&self.code {
                if self.history_tabs.len()>=8{self.history_tabs.clear();}
                self.history_tabs.insert(format!("changes:{}",navigation::code_key(code)),self.document.clone());
            }
        }
        if self.document.kind==View::Timeline {
            if let Some(target)=&self.document.target {
                if self.history_tabs.len()>=8{self.history_tabs.clear();}
                self.history_tabs.insert(target.label(),self.document.clone());
            }
        }
        let target=self.document.target.clone().or_else(||self.explorer.target());
        match view {
            View::DecisionOverview => self.decision_home(),
            View::Diff | View::Recorded => {
                if let Some(target)=target {
                    let code=document::preview(&self.review,target);
                    let key=format!("changes:{}",navigation::code_key(&code));
                    let saved=self.history_tabs.get(&key).cloned();
                    self.code=Some(code);
                    if let Some(doc)=saved{self.document=doc;self.focus=Focus::Reader;}else{self.show_notes();}
                } else {self.open(document::empty(&self.review));}
            }
            View::Explanation => {
                if let Some(target)=target {
                    let code=document::preview(&self.review,target.clone());
                    let key=navigation::code_key(&code);
                    if let Some(answer)=self.answer_for(&key){self.open(document::explanation(answer));}
                    else{self.open(document::explanation_prompt(target));}
                    self.code=Some(code);
                }
            }
            View::Timeline => {
                if let Some(target)=target {
                    let code=document::preview(&self.review,target.clone());
                    let doc=self.history_tabs.get(&target.label()).cloned().unwrap_or_else(||history_views::context(&self.review,&self.sessions,target,false));
                    self.open(doc);
                    self.code=Some(code);
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn mark(&mut self) {
        if let Some(target) = self.required_target() {
            if !explorer::files(&self.review)
                .into_iter()
                .any(|c| c["file"] == target.file)
            {
                self.message("Only changed files can be marked reviewed");
                return;
            }
            if self.explorer.reviewed.remove(&target.file) {
                self.message(format!("{} · needs review", target.file));
            } else {
                self.explorer.reviewed.insert(target.file.clone());
                self.message(format!(
                    "{} · marked reviewed for this session",
                    target.file
                ));
            }
        }
    }
    fn refresh(&mut self) -> Result<()> {
        let review = service::review(
            &self.root,
            &service::ReviewOptions {
                source: self.source.clone(),
                ..Default::default()
            },
        )?;
        self.sessions = crate::history::saved(&review)?;
        self.history_tabs.clear();
        self.explorer.refresh(&self.review, &review);
        self.review = review;
        // Reading history belongs to the previous snapshot.
        self.back.clear();
        self.views.clear();
        self.code = None;
        self.document = document::empty(&self.review);
        self.select_latest_work();
        self.decision_brief=crate::decisions::saved(&self.root,&self.decision_scope())?.map(Arc::new);
        self.review["recent_code"]=serde_json::json!([]);self.explorer.rebuild(&self.review);
        self.decision_home();
        self.back.clear();
        self.message("Changes refreshed");
        Ok(())
    }
    fn scroll(&mut self, delta: isize) {
        let max = self.scroll_max;
        let doc = self.active_document();
        doc.source_selection = None;
        doc.scroll = doc
            .scroll
            .saturating_add_signed(delta.clamp(i16::MIN as isize, i16::MAX as isize) as i16)
            .min(max);
    }
    fn active_document(&mut self) -> &mut Document {
        &mut self.document
    }
    fn select_source(&mut self, delta: isize) {
        if self.document.sources.is_empty() {
            return;
        }
        self.document.source_selection = Some(
            self.document
                .source_selection
                .unwrap_or(0)
                .saturating_add_signed(delta)
                .min(self.document.sources.len() - 1),
        );
        self.focus = Focus::Reader;
    }
    fn key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Result<bool> {
        if modifiers.contains(KeyModifiers::CONTROL)
            && matches!(code, KeyCode::Char('q') | KeyCode::Char('c'))
        {
            self.cancel();
            return Ok(true);
        }
        if self.editing.is_some() {
            match code {
                KeyCode::Esc => {
                    if self.editing == Some(Input::Filter) {
                        self.explorer.filter.clear();
                        self.explorer.rebuild(&self.review);
                    }
                    self.editing = None;
                    self.draft = None;
                    self.input.clear();
                }
                KeyCode::Enter => self.command()?,
                KeyCode::Backspace => {
                    self.input.pop();
                    self.filter_input();
                }
                KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
                    self.input.clear();
                    self.filter_input();
                }
                KeyCode::Char(c)
                    if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.input.push(c);
                    self.filter_input();
                }
                _ => {}
            }
            return Ok(false);
        }
        if modifiers.contains(KeyModifiers::CONTROL) && matches!(code,KeyCode::Left|KeyCode::Right) {
            let tabs=[View::DecisionOverview,View::Sessions];
            let current=tabs.iter().position(|v|*v==self.section()).unwrap_or(0);
            let next=if code==KeyCode::Right{(current+1)%tabs.len()}else{(current+tabs.len()-1)%tabs.len()};
            self.change_view(tabs[next])?;return Ok(false);
        }
        if modifiers.contains(KeyModifiers::ALT) && matches!(code,KeyCode::Left|KeyCode::Right) {
            if let Some(page)=&self.document.pagination {
                let index=if code==KeyCode::Right{page.index().saturating_add(1)}else{page.index().saturating_sub(1)};
                self.follow(Link::Page(index))?;
            }
            return Ok(false);
        }
        let selection_before = self.explorer.selected().map(|r| r.key.clone());
        match code {
            KeyCode::Char('q') => {
                self.cancel();
                return Ok(true);
            }
            KeyCode::Char('/') => self.edit(Input::Command),
            KeyCode::Char('g') => self.open_commits()?,
            KeyCode::Char('[') => self.adjust_pane(-3)?,
            KeyCode::Char(']') => self.adjust_pane(3)?,
            KeyCode::Char('i') => {
                if self.document.artifact.as_ref().is_some_and(|a|a["context"]=="decision_brief" || a["context"]=="session_work") {
                    self.message("Inspect captured evidence, or open Decisions to request an assessment");
                } else if self.document.artifact.is_some() {
                    self.focus = Focus::Reader;
                    self.edit(Input::Question);
                } else {
                    self.message("Select a file to ask a question");
                }
            }
            KeyCode::Char('f') => self.edit(Input::Filter),
            // Tab toggles Changes and Notes; Shift+Tab returns to the file tree.
            KeyCode::Tab | KeyCode::BackTab if (self.selected_work.is_some() || matches!(self.document.kind,View::DecisionOverview|View::SessionWork|View::Sessions)) && self.section()!=View::Recorded => {
                self.change_view(View::Sessions)?;
            }
            KeyCode::BackTab => {
                self.change_view(View::Recorded)?;
                self.sidebar = true;
                self.focus = Focus::Files;
            }
            KeyCode::Tab => {
                if self.section()!=View::Recorded {self.change_view(View::Recorded)?;}
                self.focus = match self.focus {
                    Focus::Files => Focus::Reader,
                    Focus::Reader => {
                        self.sidebar = true;
                        Focus::Files
                    }
                };
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.focus == Focus::Files {
                    self.explorer.step(-1);
                } else if self.focus == Focus::Reader
                    && (self.document.source_selection.is_some()
                        || self.document.kind == View::Commits)
                {
                    self.select_source(-1);
                } else {
                    self.scroll(-1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.focus == Focus::Files {
                    self.explorer.step(1);
                } else if self.focus == Focus::Reader
                    && (self.document.source_selection.is_some()
                        || self.document.kind == View::Commits)
                {
                    self.select_source(1);
                } else {
                    self.scroll(1);
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.focus == Focus::Files {
                    self.explorer.expand(&self.review);
                } else if self.document.code() || !self.document.code_gutters.is_empty() {
                    let doc = self.active_document();
                    doc.horizontal = doc.horizontal.saturating_add(4);
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if self.focus == Focus::Files {
                    self.explorer.collapse(&self.review);
                } else {
                    let doc = self.active_document();
                    doc.horizontal = doc.horizontal.saturating_sub(4);
                }
            }
            KeyCode::Char(' ') if self.focus == Focus::Files => self.explorer.toggle(&self.review),
            KeyCode::PageDown => self.scroll(self.page_size as isize),
            KeyCode::PageUp => self.scroll(-(self.page_size as isize)),
            KeyCode::Home => {
                if self.focus == Focus::Files {
                    self.explorer.step(isize::MIN);
                } else if self.document.kind == View::Commits {
                    self.select_source(isize::MIN);
                } else {
                    self.active_document().scroll = 0;
                }
            }
            KeyCode::End => {
                if self.focus == Focus::Files {
                    self.explorer.step(isize::MAX);
                } else if self.document.kind == View::Commits {
                    self.select_source(isize::MAX);
                } else {
                    self.document.scroll = self.scroll_max;
                }
            }
            KeyCode::Char('b') if self.selected_work.is_some() || self.section()!=View::Recorded => self.session_picker(),
            KeyCode::Char('b') => {
                if self.section()!=View::Recorded {self.change_view(View::Recorded)?;self.sidebar=false;}
                self.sidebar = !self.sidebar;
                self.focus = if self.sidebar {
                    Focus::Files
                } else {
                    Focus::Reader
                };
            }
            KeyCode::Char('z') => {
                let codes:Vec<_>=self.document.sources.iter().filter_map(|(_,link)|if let Link::Disclosure(id)=link{id.starts_with("code-block-").then_some(id.clone())}else{None}).collect();
                let expand=codes.iter().any(|id|!self.document.expanded.contains(id));
                for id in &codes{if self.document.expanded.contains(id)!=expand{self.document.toggle_disclosure(id);}}
                self.document.source_selection=None;
                self.message(if codes.is_empty(){"No code blocks in this view"}else if expand{"Code blocks expanded"}else{"Code blocks collapsed"});
            }
            KeyCode::Char('d') => self.decision_home(),
            KeyCode::Char('e') if matches!(self.document.kind,View::DecisionOverview|View::SessionWork) => self.discover_decisions(),
            KeyCode::Char('e') => self.why_change(false),
            KeyCode::Char('w') => {
                self.brief = !self.brief;
                self.rebuild_reader();
            }
            KeyCode::Char('o') if self.selected_work.is_some() => self.session_flow(),
            KeyCode::Char('o') => self.change_view(View::Recorded)?,
            KeyCode::Char('v') if self.session_context() => self.decision_home(),
            KeyCode::Char('v') => self.change_view(View::Explanation)?,
            KeyCode::Char('t') => self.change_view(View::Sessions)?,
            KeyCode::Char('x') => self.cancel(),
            KeyCode::Char('s') if !self.document.sources.is_empty() => self.select_source(0),
            KeyCode::Char('R') if matches!(self.document.kind,View::DecisionOverview|View::SessionWork) => self.discover_decisions(),
            KeyCode::Char('R') => self.why_change(true),
            KeyCode::Char('p') => self.saved_explanation()?,
            KeyCode::Char('m') => self.mark(),
            KeyCode::Char('r') => {
                if self.job.is_none() {
                    self.refresh()?;
                } else {
                    self.message("Wait for enrichment or press x to cancel before refreshing");
                }
            }
            KeyCode::Enter => {
                if self.focus == Focus::Reader && self.document.kind == View::Commits {
                    if !self.document.commits.is_empty() {
                        self.open_evidence(self.document.source_selection.unwrap_or(0))?;
                    }
                } else if self.focus == Focus::Reader && self.document.source_selection.is_some() {
                    let link = self.document.sources[self.document.source_selection.unwrap()].1.clone();
                    self.follow(link)?;
                } else if self.focus == Focus::Reader
                    && matches!(self.document.kind,View::SessionWork|View::SessionImplementation|View::SessionComparison|View::DecisionOverview|View::DecisionDetail|View::Recorded|View::Explanation|View::Sessions|View::Timeline|View::Decisions|View::Session|View::Turn|View::Evidence|View::Original|View::Commit)
                    && !self.document.sources.is_empty()
                {
                    self.select_source(0);
                } else if self.focus == Focus::Files
                    && self
                        .explorer
                        .selected()
                        .is_some_and(|r| matches!(r.kind,Kind::Folder|Kind::Section))
                {
                    self.explorer.toggle(&self.review);
                } else {
                    // Enter in the tree moves into the reader for this file.
                    if self.focus == Focus::Files {
                        self.preview_selection();
                    }
                    self.focus = Focus::Reader;
                }
            }
            KeyCode::Char(c @ '1'..='9') => {
                let index=c.to_digit(10).unwrap() as usize-1;
                if self.document.kind==View::DecisionOverview {
                    self.follow(Link::Decision(index))?;
                } else if self.document.kind==View::DecisionDetail {
                    let link=self.document.sources.iter().filter(|(_,link)|matches!(link,Link::Source(_))).nth(index).map(|(_,link)|link.clone()).ok_or_else(||anyhow::anyhow!("No evidence at this position for this decision"))?;
                    self.follow(link)?;
                } else {self.open_evidence(index)?;}
            }
            KeyCode::Esc => {
                if self.focus == Focus::Reader
                    && self.document.kind != View::Commits
                    && self.document.source_selection.take().is_some()
                {
                    // Leave the source list and return to reading this explanation.
                } else if self.focus == Focus::Files && !self.explorer.filter.is_empty() {
                    self.explorer.filter.clear();
                    self.explorer.rebuild(&self.review);
                } else if let Some((mut doc, focus, code)) = self.back.pop() {
                    self.check_freshness(&mut doc);
                    self.document = doc;
                    self.code = code;
                    self.focus = if !self.sidebar { Focus::Reader } else { focus };
                }
            }
            KeyCode::Char('?') | KeyCode::F(1) => self.open(document::help()),
            _ => {}
        }
        if self.focus == Focus::Files
            && self.explorer.selected().map(|r| r.key.clone()) != selection_before
        {
            self.preview_selection();
        }
        Ok(false)
    }
    fn mouse(&mut self, event: MouseEvent) -> Result<()> {
        let point = Position::new(event.column, event.row);
        if self.editing.is_some() {
            return Ok(());
        }
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if self.areas.file_divider.contains(point) {
                    self.dragging = Some(Divider::Files);
                } else if let Some((_, link)) = self
                    .areas
                    .sources
                    .iter()
                    .find(|(area, _)| area.contains(point))
                {
                    self.follow(link.clone())?;
                } else if let Some((_, view, focus)) = self
                    .areas
                    .tabs
                    .iter()
                    .find(|(area, _, _)| area.contains(point))
                {
                    let (view, focus) = (*view, *focus);
                    self.focus = focus;
                    self.change_view(view)?;
                } else if self.areas.files.contains(point) {
                    let index =
                        self.explorer.state.offset() + usize::from(event.row - self.areas.files.y);
                    if index < self.explorer.rows.len() {
                        self.explorer.state.select(Some(index));
                        self.focus = Focus::Files;
                        // A click previews the file (or toggles a folder) without leaving the tree.
                        if self.explorer.selected().is_some_and(|r| matches!(r.kind,Kind::Folder|Kind::Section)) {
                            self.explorer.toggle(&self.review);
                        }
                        self.preview_selection();
                    }
                } else if self.areas.reader.contains(point) {
                    self.focus = Focus::Reader;
                } else if self.areas.input.contains(point) {
                    self.focus = Focus::Reader;
                    self.edit(Input::Question);
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(divider) = self.dragging {
                    self.resize_pane(divider, event.column);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some(divider) = self.dragging.take() {
                    self.resize_pane(divider, event.column);
                    self.save_layout()?;
                }
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let delta = if event.kind == MouseEventKind::ScrollDown {
                    3
                } else {
                    -3
                };
                if self.areas.files.contains(point) {
                    self.focus = Focus::Files;
                    self.explorer.step(delta);
                    self.preview_selection();
                } else if self.areas.reader.contains(point) {
                    self.focus = Focus::Reader;
                    self.scroll(delta);
                }
            }
            _ => {}
        }
        Ok(())
    }
}
struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen
        );
    }
}
pub fn run(root: &Path) -> Result<()> {
    ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        "wy is an interactive app; run it in a terminal"
    );
    let mut app = Workspace::new(root)?;
    terminal::enable_raw_mode()?;
    let _guard = TerminalGuard;
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let result = (|| -> Result<()> {
        let mut dirty = true;
        loop {
            let was_running = app.job.is_some();
            app.poll();
            if dirty || was_running {
                terminal.draw(|frame| app.draw(frame))?;
                dirty = false;
            }
            if event::poll(Duration::from_millis(if app.job.is_some() {
                100
            } else {
                250
            }))? {
                let event = event::read()?;
                if matches!(
                    event,
                    Event::Mouse(MouseEvent {
                        kind: MouseEventKind::Moved,
                        ..
                    }) | Event::Key(event::KeyEvent {
                        kind: KeyEventKind::Release,
                        ..
                    })
                ) {
                    continue;
                }
                dirty = true;
                let result = match event {
                    Event::Key(key) if key.kind != KeyEventKind::Release => {
                        app.key(key.code, key.modifiers)
                    }
                    Event::Mouse(mouse) => app.mouse(mouse).map(|_| false),
                    Event::Paste(text) if app.editing.is_some() => {
                        app.input
                            .extend(text.chars().map(|c| if c.is_control() { ' ' } else { c }));
                        app.filter_input();
                        Ok(false)
                    }
                    _ => Ok(false),
                };
                match result {
                    Ok(true) => break,
                    Ok(false) => {}
                    Err(error) => app.fail(format!("{error:#}")),
                }
            }
        }
        Ok(())
    })();
    app.cancel();
    if let Some(job) = app.job.take() {
        let _ = job.handle.join();
    }
    terminal.show_cursor()?;
    result
}

#[cfg(test)]
mod tests;
