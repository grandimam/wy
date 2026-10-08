//! Read-only terminal workspace. Model requests run only after explicit input.
use anyhow::{Result, ensure};
use crossterm::{event::{self, Event, KeyCode, KeyModifiers, KeyEventKind}, execute, terminal::{self, EnterAlternateScreen, LeaveAlternateScreen}};
use ratatui::{prelude::*, widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap}};
use serde_json::Value;
use std::{io::{self, IsTerminal}, path::{Path, PathBuf}, sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc}, thread, time::Duration};
use crate::{arr, s, n, agent::Cancel, presentation, reasoning, repository, security, service};

#[derive(Clone)]
struct Target { label: String, file: String, symbol: Option<String> }
enum Update { Progress(String), Done(Result<Value>) }
struct Job { receiver: mpsc::Receiver<Update>, cancel: Cancel, handle: thread::JoinHandle<()> }
struct Workspace {
    root: PathBuf, review: Value, targets: Vec<Target>, selected: usize,
    body: String, scroll: u16, agent: String, source: String,
    input: String, editing: bool, sidebar: bool, status: String,
    artifact: Option<Value>, back: Vec<String>, job: Option<Job>,
}
impl Workspace {
    fn new(root: &Path) -> Result<Self> {
        let review = service::review(root, &service::ReviewOptions { source: "both".into(), ..Default::default() })?;
        let mut app = Self { root: root.into(), review, targets: vec![], selected: 0,
            body: "Explore a changed file or function, then press e to explain it or w to investigate its design.\n\nPress a to explain all changes. Agent calls use your existing account.\n\nPress d for offline detected choices, v for source, Enter for diff, ? for help.".into(),
            scroll: 0, agent: "codex".into(), source: "both".into(), input: String::new(), editing: false,
            sidebar: true, status: "Offline review ready".into(), artifact: None, back: vec![], job: None };
        app.rebuild(); Ok(app)
    }
    fn rebuild(&mut self) {
        self.targets.clear();
        for change in arr(&self.review["changes"]) {
            let file = s(&change["file"]).to_owned();
            self.targets.push(Target { label: file.clone(), file: file.clone(), symbol: None });
            for symbol in arr(&change["symbols"]) {
                self.targets.push(Target { label: format!("  {} :{}", s(&symbol["symbol"]), n(&symbol["start_line"])), file: file.clone(), symbol: Some(s(&symbol["symbol"]).into()) });
            }
        }
        self.selected = self.selected.min(self.targets.len().saturating_sub(1));
    }
    fn show(&mut self, text: String) { self.back.push(self.body.clone()); self.body = text; self.scroll = 0; }
    fn start(&mut self, all: bool, design: bool, question: Option<String>) {
        if self.job.is_some() { self.status = "A request is running; Esc cancels it".into(); return; }
        let selected = if all { None } else { self.targets.get(self.selected).cloned() };
        if !all && selected.is_none() { self.status = "Select a file first".into(); return; }
        let options = reasoning::Options {
            agent: self.agent.clone(), source: self.source.clone(),
            question: question.unwrap_or_else(|| if design { "Why this design?".into() } else { reasoning::prompt("explain").into() }),
            target: if design { selected.as_ref().map(|t| format!("{}:{}",t.file,t.symbol.as_deref().unwrap_or("1"))) } else { None },
            file: selected.map(|t| t.file),
        };
        let root = self.root.clone(); let cancel = Arc::new(AtomicBool::new(false)); let worker_cancel = cancel.clone();
        let (sender, receiver) = mpsc::channel();
        let handle = thread::spawn(move || {
            let result = reasoning::run(&root, &options, &worker_cancel, |p| { let _ = sender.send(Update::Progress(p.into())); });
            let _ = sender.send(Update::Done(result));
        });
        self.job = Some(Job { receiver, cancel, handle }); self.status = "Preparing explanation… Esc cancels".into();
    }
    fn poll(&mut self) {
        let mut done = false;
        while let Some(update) = self.job.as_ref().and_then(|j| j.receiver.try_recv().ok()) {
            match update {
                Update::Progress(p) => self.status = p,
                Update::Done(result) => { done = true; match result {
                    Ok(artifact) => { self.show(presentation::reasoning_text(&artifact)); self.artifact = Some(artifact); self.status = "Explanation saved · 1–9 opens evidence".into(); },
                    Err(e) => self.status = security::redact(&format!("{e:#}")),
                }}
            }
        }
        if done { if let Some(job) = self.job.take() { let _ = job.handle.join(); } }
    }
    fn cancel(&mut self) { if let Some(j) = &self.job { j.cancel.store(true, Ordering::Relaxed); self.status = "Cancelling…".into(); } }
    fn command(&mut self) -> Result<()> {
        let input = self.input.trim().to_owned(); self.input.clear(); self.editing = false;
        if input.is_empty() { return Ok(()); }
        let (name, rest) = input.split_once(' ').unwrap_or((&input, ""));
        match name {
            "/reason" => self.start(true, false, (!rest.is_empty()).then(|| rest.into())),
            "/why" => {
                let (target, question) = rest.split_once(' ').unwrap_or((rest, "Why this design?"));
                let resolved = reasoning::resolve_target(&self.root, target)?;
                self.targets.push(Target { label: target.into(), file: s(&resolved["file"]).into(), symbol: Some(target.rsplit_once(':').map(|(_,s)|s).unwrap_or("1").into()) });
                self.selected = self.targets.len()-1; self.start(false, true, Some(question.into()));
            }
            "/agent" => { ensure!(["codex","claude"].contains(&rest), "Use /agent codex or /agent claude"); self.agent = rest.into(); }
            "/source" => { crate::history::valid_source(rest)?; self.source = rest.into(); }
            "/cancel" => self.cancel(),
            "/ask" => self.follow_up(rest),
            _ if input.starts_with('/') => self.status = "Commands: /reason QUESTION, /why FILE:SYMBOL QUESTION, /ask QUESTION, /agent codex|claude, /source both|codex|claude|none, /cancel".into(),
            _ => self.follow_up(&input),
        } Ok(())
    }
    fn follow_up(&mut self, question: &str) {
        let mut question = question.to_owned();
        if let Some(a) = &self.artifact {
            question = format!("Previous question: {}\nPrevious assessment:\n{}\n\nFollow-up: {question}",s(&a["packet"]["question"]), presentation::reasoning_text(a));
            // Retain the previous target rather than the current sidebar selection.
            let target = a["packet"]["focus_target"]["target"].as_str().map(str::to_owned);
            let file = a["packet"]["file"].as_str().map(str::to_owned);
            if let Some(file) = target.as_ref().map(|t|t.split(':').next().unwrap_or(t).to_owned()).or(file) {
                self.targets.push(Target { label: file.clone(), file, symbol: target.as_ref().and_then(|t|t.rsplit_once(':').map(|(_,s)|s.into())) });
                self.selected = self.targets.len()-1; self.start(false, target.is_some(), Some(question)); return;
            }
        }
        self.start(true, false, Some(question));
    }
    fn key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Result<bool> {
        if modifiers.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('q') | KeyCode::Char('c')) { self.cancel(); return Ok(true); }
        if self.editing {
            match code { KeyCode::Esc => { self.editing=false; self.input.clear(); }, KeyCode::Enter => self.command()?, KeyCode::Backspace => {self.input.pop();}, KeyCode::Char(c) => self.input.push(c), _=>{} } return Ok(false);
        }
        match code {
            KeyCode::Char('q') => { self.cancel(); return Ok(true); },
            KeyCode::Char('/') => { self.editing=true; self.input="/".into(); },
            KeyCode::Char('i') => self.editing=true,
            KeyCode::Up | KeyCode::Char('k') => self.selected=self.selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.selected=(self.selected+1).min(self.targets.len().saturating_sub(1)),
            KeyCode::PageDown => self.scroll=self.scroll.saturating_add(12),
            KeyCode::PageUp => self.scroll=self.scroll.saturating_sub(12),
            KeyCode::Char('b') => self.sidebar=!self.sidebar,
            KeyCode::Char('a') => self.start(true,false,None),
            KeyCode::Char('e') => self.start(false,false,None),
            KeyCode::Char('w') => self.start(false,true,None),
            KeyCode::Char('d') => { self.review=service::load(&self.root)?; self.show(presentation::render(&self.review)); },
            KeyCode::Char('r') if self.job.is_none() => {
                self.review=service::review(&self.root,&service::ReviewOptions {source:self.source.clone(),..Default::default()})?;
                self.rebuild(); self.status="Offline review refreshed".into();
            }
            KeyCode::Char('v') => if let Some(t)=self.targets.get(self.selected) {
                let raw=security::read_source(&self.root,&t.file).ok_or_else(||anyhow::anyhow!("Source unavailable"))?;
                self.show(format!("{}\n\n{}", t.file, security::redact(&raw).lines().enumerate().map(|(i,l)|format!("{:4}  {l}",i+1)).collect::<Vec<_>>().join("\n")));
            },
            KeyCode::Enter => if let Some(t)=self.targets.get(self.selected) {
                let text=arr(&self.review["changes"]).iter().find(|c|c["file"]==t.file).map(|c|s(&c["diff"]).to_owned()).unwrap_or_else(||"No captured diff".into()); self.show(text);
            },
            KeyCode::Char(c @ '1'..='9') => if let Some(a)=&self.artifact {
                if let Some(e)=presentation::citations(a).get(c.to_digit(10).unwrap() as usize-1) { self.show(serde_json::to_string_pretty(e)?); }
            },
            KeyCode::Esc => { if self.job.is_some() {self.cancel();} else if let Some(body)=self.back.pop(){self.body=body;self.scroll=0;} },
            KeyCode::Char('?') | KeyCode::F(1) => self.show("wy · Help\n\nj/k or arrows: select file or function\nEnter: diff · v: source · d: offline choices\na: explain all changes · e: explain selected file · w: why this design\n1–9: inspect explanation evidence · Esc: back / cancel\nPageUp/PageDown: scroll · b: toggle files\ni: ask follow-up · /: command input\nr: refresh review offline · q / Ctrl+Q: quit\n\n/agent codex|claude\n/source both|codex|claude|none\n/reason QUESTION\n/why FILE:SYMBOL QUESTION\n/ask QUESTION\n/cancel\n\nAgent requests are opt-in and may consume account usage. Source browsing and review stay offline.".into()),
            _=>{}
        } Ok(false)
    }
    fn draw(&self, frame: &mut Frame) {
        let rows=Layout::vertical([Constraint::Length(1),Constraint::Min(3),Constraint::Length(3),Constraint::Length(2)]).split(frame.area());
        frame.render_widget(Paragraph::new(format!(" wy  ·  {}  ·  agent: {}  ·  history: {}", self.root.display(),self.agent,self.source)).style(Style::default().fg(Color::Cyan)),rows[0]);
        let cols=Layout::horizontal([Constraint::Length(if self.sidebar {32}else{0}),Constraint::Min(20)]).split(rows[1]);
        if self.sidebar {
            let items=self.targets.iter().map(|t| ListItem::new(t.label.as_str())).collect::<Vec<_>>();
            let mut state=ListState::default().with_selected((!items.is_empty()).then_some(self.selected));
            frame.render_stateful_widget(List::new(items).block(Block::default().title("Changed files / symbols").borders(Borders::ALL)).highlight_style(Style::default().bg(Color::DarkGray).fg(Color::Cyan)),cols[0],&mut state);
        }
        frame.render_widget(Paragraph::new(self.body.as_str()).wrap(Wrap{trim:false}).scroll((self.scroll,0)).block(Block::default().title("Explanation / evidence").borders(Borders::ALL)),cols[1]);
        frame.render_widget(Paragraph::new(if self.editing {self.input.as_str()}else{"i: ask a follow-up · /: commands"}).block(Block::default().title(if self.editing{"Enter sends · Esc cancels input"}else{"Question"}).borders(Borders::ALL)),rows[2]);
        frame.render_widget(Paragraph::new(format!("{}\n?: help · a: explain all · e: explain file · w: why · Esc: back/cancel · q: quit",self.status)),rows[3]);
    }
}
struct TerminalGuard;
impl Drop for TerminalGuard { fn drop(&mut self) { let _=terminal::disable_raw_mode(); let _=execute!(io::stdout(),LeaveAlternateScreen); } }
pub fn run(root:&Path)->Result<()> {
    ensure!(io::stdin().is_terminal()&&io::stdout().is_terminal(),"The workspace requires an interactive terminal; use wy review --json for scripts");
    let mut app=Workspace::new(root)?;
    terminal::enable_raw_mode()?; let _guard=TerminalGuard;
    execute!(io::stdout(),EnterAlternateScreen)?;
    let mut terminal=Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let result=(||->Result<()> { loop {
        app.poll(); terminal.draw(|f|app.draw(f))?;
        if event::poll(Duration::from_millis(80))? {
            if let Event::Key(key)=event::read()? { if key.kind==KeyEventKind::Press {
                match app.key(key.code,key.modifiers) {Ok(true)=>break,Ok(false)=>{},Err(e)=>app.status=security::redact(&format!("{e:#}"))}
            }}
        }
    } Ok(()) })();
    app.cancel();
    if let Some(job)=app.job.take() { let _=job.handle.join(); }
    terminal.show_cursor()?; result
}
