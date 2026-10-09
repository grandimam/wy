use super::*;
use ratatui::backend::TestBackend;
use serde_json::json;

fn review() -> Value {
    json!({"changes":[
        {"file":"README.md","symbols":[],"added_lines":[1,2],"removed_line_count":1,"diff":"--- a/README.md\n+++ b/README.md\n@@ -1 +1,2 @@\n-old\n+new\n+guide"},
        {"file":"src/cache.rs","symbols":[
            {"symbol":"Cache","start_line":3,"end_line":6},
            {"symbol":"Cache","start_line":8,"end_line":90},
            {"symbol":"Cache.get","start_line":10,"end_line":25},
            {"symbol":"Cache.refresh","start_line":40,"end_line":65}
        ],"added_lines":[12,13,14,41],"removed_line_count":2,"diff":"--- a/src/cache.rs\n+++ b/src/cache.rs\n@@ -10,3 +10,5 @@\n fn get() {\n-    fetch()\n+    cache.get(key)\n+        .unwrap_or_else(fetch)\n }"},
        {"file":"src/http/client.rs","symbols":[{"symbol":"request","start_line":2,"end_line":12}],"added_lines":[2,3,4],"removed_line_count":0,"diff":"+fn request() {}"},
        {"file":"tests/cache.rs","symbols":[],"added_lines":[1,2,3,4,5,6],"removed_line_count":0,"diff":"+test"}
    ],"decisions":[{"question":"Why cache these responses?","provenance":"unexplained","location":{"file":"src/cache.rs","start_line":12},"explanation":"The response cache is visible, but the reason for its lifetime is not established.","assumptions":["Responses remain valid for the cache lifetime."],"unresolved_questions":["How are stale entries invalidated?"],"evidence":[]}],
    "sessions":[{"agent":"codex"}],"warnings":["No pre-session baseline: existing edits may be included; authorship is unknown."],
    "file_hashes":{"src/cache.rs":"original","src/http/client.rs":"original"}})
}
fn workspace() -> Workspace {
    Workspace::from_review(Path::new("/example/payments"), review())
}
fn press(app: &mut Workspace, key: KeyCode) {
    app.key(key, KeyModifiers::NONE).unwrap();
}
fn select(app: &mut Workspace, key: &str) {
    let index = app.explorer.rows.iter().position(|r| r.key == key).unwrap();
    app.explorer.state.select(Some(index));
    app.focus = Focus::Files;
}
fn screen(app: &mut Workspace, width: u16, height: u16) -> (String, Terminal<TestBackend>) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .chunks(width as usize)
        .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    (text, terminal)
}
fn artifact(file: Option<&str>) -> Arc<Value> {
    Arc::new(
        json!({"id":"explanation-test", "packet":{"focus_file":file,"question":"What changed?","evidence":[{"id":"code-1","kind":"code","file":"src/cache.rs","start_line":10,"text":"fn get() { cache.get(key) }"}]}, "explanation":{
            "title":"Cache repeated requests with an explicit lifetime",
            "answer":{"text":"Repeated requests can reuse a cached response.","basis":"observed","evidence_ids":["code-1"]},
            "problem":{"text":"The original requirement is not established.","basis":"unknown","evidence_ids":[]},
            "before":{"text":"Each request fetched its response.","basis":"observed","evidence_ids":["code-1"]},
            "after":{"text":"A cache hit returns an existing response.","basis":"observed","evidence_ids":["code-1"]},
            "judgments":[{"choice":"Keep responses in memory","reason":"This may reduce repeated work, but the original reason is missing.","status":"inferred","evidence_ids":["code-1"],"quote":""}],
            "steps":[],"tradeoffs":[],"checks":[{"text":"Check whether an expired entry triggers a fresh request.","basis":"proposed","evidence_ids":["code-1"]}],"unknowns":["The intended cache lifetime is not recorded."]
        }}),
    )
}

#[test]
fn tree_groups_files_collapses_symbols_and_preserves_selection() {
    let mut app = workspace();
    assert!(app.explorer.rows.iter().all(|r| r.kind != Kind::Symbol));
    assert_eq!(
        app.explorer
            .rows
            .iter()
            .map(|r| r.key.as_str())
            .collect::<Vec<_>>(),
        [
            "src/",
            "src/http/",
            "src/http/client.rs",
            "src/cache.rs",
            "tests/",
            "tests/cache.rs",
            "README.md"
        ]
    );
    select(&mut app, "src/cache.rs");
    assert_eq!(
        app.options(app.explorer.target(), true, None)
            .target
            .as_deref(),
        Some("src/cache.rs")
    );
    press(&mut app, KeyCode::Right);
    assert_eq!(
        app.explorer
            .rows
            .iter()
            .filter(|r| r.kind == Kind::Symbol)
            .count(),
        4
    );
    press(&mut app, KeyCode::Right);
    assert_eq!(app.explorer.target().unwrap().selector(), "src/cache.rs:3");
    press(&mut app, KeyCode::Down);
    assert_eq!(app.explorer.target().unwrap().selector(), "src/cache.rs:8");
    let options = app.options(app.explorer.target(), true, None);
    assert_eq!(options.target.as_deref(), Some("src/cache.rs:8"));
    select(&mut app, "src/cache.rs");
    press(&mut app, KeyCode::Left);
    assert!(app.explorer.rows.iter().all(|r| r.kind != Kind::Symbol));
    app.explorer.rebuild(&app.review);
    assert_eq!(app.explorer.target().unwrap().file, "src/cache.rs");
    press(&mut app, KeyCode::Left);
    assert_eq!(app.explorer.selected().unwrap().key, "src/");
    press(&mut app, KeyCode::Left);
    assert!(!app.explorer.rows.iter().any(|r| r.key == "src/cache.rs"));
}
#[test]
fn filter_finds_files_inside_closed_folders_and_recovers_empty_results() {
    let mut app = workspace();
    select(&mut app, "src/");
    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Char('f'));
    for c in "HTTP/CLIENT".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    assert!(
        app.explorer
            .rows
            .iter()
            .any(|r| r.key == "src/http/client.rs")
    );
    press(&mut app, KeyCode::Char('x'));
    assert!(app.explorer.rows.is_empty());
    assert!(screen(&mut app, 100, 30).0.contains("No matching files"));
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Esc);
    assert!(app.explorer.filter.is_empty());
    assert!(!app.explorer.rows.is_empty());
    assert!(
        !app.explorer
            .rows
            .iter()
            .any(|r| r.key == "src/http/client.rs")
    );
}
#[test]
fn reading_focus_scroll_and_back_restore_the_actual_document() {
    let mut app = workspace();
    select(&mut app, "src/cache.rs");
    screen(&mut app, 100, 24);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.focus, Focus::Code);
    assert_eq!(app.code.as_ref().unwrap().kind, View::Diff);
    let selected = app.explorer.state.selected();
    screen(&mut app, 100, 24);
    press(&mut app, KeyCode::Down);
    assert_eq!(app.explorer.state.selected(), selected);
    let mut doc = document::help();
    doc.scroll = 8;
    app.open(doc);
    app.open(document::explanation(artifact(Some("src/cache.rs"))));
    press(&mut app, KeyCode::Char('1'));
    assert_eq!(app.document.kind, View::Evidence);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.document.kind, View::Explanation);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.document.scroll, 8);
    assert_eq!(app.document.kind, View::Help);
    screen(&mut app, 100, 24);
    press(&mut app, KeyCode::End);
    press(&mut app, KeyCode::PageDown);
    assert_eq!(app.document.scroll, app.scroll_max);
    press(&mut app, KeyCode::Home);
    assert_eq!(app.document.scroll, 0);
}
#[test]
fn selection_previews_the_diff_without_requests_or_history_noise() {
    let mut app = workspace();
    assert_eq!(app.document.kind, View::Recorded);
    assert_eq!(app.document.target, app.explorer.target());
    press(&mut app, KeyCode::Down);
    assert_eq!(app.document.target.as_ref().unwrap().file, "src/cache.rs");
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Right);
    assert_eq!(app.document.target.as_ref().unwrap().line, 3);
    assert!(app.document.target.as_ref().unwrap().symbol.is_some());
    assert!(app.job.is_none());
    assert!(app.back.is_empty());
    for key in ['a', 'o', 'v'] {
        press(&mut app, KeyCode::Char(key));
    }
    assert!(app.job.is_none());
    assert_eq!(app.document.kind, View::Recorded);
}
#[test]
fn why_requests_use_the_selected_change_and_refresh_the_original_scope() {
    let mut app = workspace();
    select(&mut app, "src/cache.rs");
    let options = app.why_options(false).unwrap();
    assert_eq!(options.target.as_deref(), Some("src/cache.rs"));
    assert!(
        options
            .question
            .starts_with(reasoning::prompt("enrich_change"))
    );
    app.open(document::explanation(artifact(None)));
    let options = app.why_options(true).unwrap();
    assert!(options.file.is_none());
    assert!(options.target.is_none());
    app.open(document::explanation(artifact(Some("README.md"))));
    app.open_evidence(0).unwrap();
    assert_eq!(app.document.target.as_ref().unwrap().file, "src/cache.rs");
    let options = app.why_options(true).unwrap();
    assert_eq!(options.target.as_deref(), Some("README.md"));
    app.focus = Focus::Files;
    assert_eq!(
        app.why_options(true).unwrap().target.as_deref(),
        Some("src/cache.rs")
    );
}
#[test]
fn diff_and_why_toggle_reuses_the_answer_for_the_exact_change() {
    let mut app = workspace();
    app.open(document::explanation(artifact(Some("src/cache.rs"))));
    app.open(document::explanation(artifact(Some("README.md"))));
    select(&mut app, "src/cache.rs");
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Char('w'));
    assert_eq!(app.document.kind, View::Explanation);
    assert_eq!(app.document.target.as_ref().unwrap().file, "src/cache.rs");
    assert!(app.job.is_none());
    press(&mut app, KeyCode::Char('1'));
    press(&mut app, KeyCode::Char('w'));
    assert_eq!(app.document.kind, View::Explanation);
    press(&mut app, KeyCode::Char('o'));
    assert_eq!(app.document.kind, View::Recorded);
    let (_, _) = screen(&mut app, 120, 32);
    let why = app
        .areas
        .tabs
        .iter()
        .find(|(_, view, _)| *view == View::Explanation)
        .unwrap()
        .0;
    app.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: why.x + 1,
        row: why.y,
        modifiers: KeyModifiers::NONE,
    })
    .unwrap();
    assert_eq!(app.document.kind, View::Explanation);
    assert!(app.job.is_none());
    assert_eq!(app.answers.len(), 2);
}
#[test]
fn recorded_reason_leads_with_the_original_quote_and_openable_reference() {
    let mut answer = (*artifact(Some("src/cache.rs"))).clone();
    let quote = "Cache repeated reads to avoid fetching the same response again.";
    answer["packet"]["evidence"].as_array_mut().unwrap().push(json!({"id":"statement-1","kind":"session","file":"conversation.jsonl","agent":"codex","role":"assistant","start_line":9,"text":quote,"provenance":{"source_type":"original_turn","basis":"test_native_event","original_refs":[]}}));
    answer["explanation"]["judgments"][0] = json!({"choice":"Cache repeated reads","reason":"The agent explicitly connected caching to avoiding repeated fetches.","status":"recorded","quote":quote,"quote_id":"statement-1","evidence_ids":["statement-1","code-1"]});
    let answer = Arc::new(answer);
    let doc = document::explanation(answer.clone());
    let text = doc
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.find(quote).unwrap() < text.find("Repeated requests can reuse").unwrap());
    assert!(text.contains(&format!("“{quote}” [1]")));
    assert!(
        !text
            .lines()
            .any(|l| ["Answer", "The request or constraint", "Tradeoffs"].contains(&l))
    );
    assert!(!text.contains("No explicit reason"));
    let source = document::evidence(answer, 0).unwrap();
    assert!(source.lines.iter().any(|line| line.to_string() == quote));
    let mut app = workspace();
    app.open(doc);
    preview("recorded-reason", &screen(&mut app, 120, 34).1);
}
#[test]
fn requests_follow_visible_scope_without_adding_explorer_rows() {
    let mut app = workspace();
    let count = app.explorer.rows.len();
    app.open(document::explanation(artifact(Some("src/cache.rs"))));
    for _ in 0..4 {
        let options = app.question_options("Why not a map?");
        assert_eq!(options.file.as_deref(), Some("src/cache.rs"));
        assert!(options.question.contains("Previous assessment"));
        assert_eq!(app.explorer.rows.len(), count);
    }
    select(&mut app, "README.md");
    assert_eq!(
        app.question_options("Why?").file.as_deref(),
        Some("README.md")
    );
    app.focus = Focus::Reader;
    let mut focused = (*artifact(Some("src/cache.rs"))).clone();
    focused["packet"]["focus_target"] = json!({"target":"src/cache.rs:Cache.refresh","file":"src/cache.rs","symbol":"Cache.refresh","start_line":40});
    app.open(document::explanation(Arc::new(focused)));
    assert_eq!(
        app.question_options("Why?").target.as_deref(),
        Some("src/cache.rs:Cache.refresh")
    );
    app.open(document::explanation(artifact(None)));
    assert!(app.question_options("Why?").file.is_none());
    app.open(document::empty(&app.review));
    assert!(app.question_options("Why?").file.is_none());
}
#[test]
fn draft_scope_survives_an_answer_arriving_while_typing() {
    let mut app = workspace();
    select(&mut app, "README.md");
    app.edit(Input::Question);
    app.input = "Why this change?".into();
    app.open(document::explanation(artifact(Some("src/cache.rs"))));
    assert_eq!(app.question_scope(), "README.md");
    assert_eq!(
        app.question_options(&app.input).file.as_deref(),
        Some("README.md")
    );
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.question_scope(), "src/cache.rs");
    assert_eq!(
        app.question_options("Why?").file.as_deref(),
        Some("src/cache.rs")
    );
}
#[test]
fn saved_explanations_and_back_check_for_edits_after_generation() {
    let root = tempfile::tempdir().unwrap();
    let status = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(root.path())
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::write(root.path().join("lib.rs"), "fn original() {}\n").unwrap();
    let saved = service::review(
        root.path(),
        &service::ReviewOptions {
            source: "none".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let mut answer = (*artifact(Some("lib.rs"))).clone();
    answer["review_id"] = saved["id"].clone();
    crate::storage::Store::open(root.path())
        .unwrap()
        .put("reasoning", "latest", &answer)
        .unwrap();
    let mut app = Workspace::from_review(root.path(), saved);
    press(&mut app, KeyCode::Char('p'));
    assert!(app.document.notice.as_ref().unwrap().0.contains("matches"));
    app.open(document::help());
    std::fs::write(root.path().join("lib.rs"), "fn changed() {}\n").unwrap();
    press(&mut app, KeyCode::Esc);
    assert!(
        app.document
            .notice
            .as_ref()
            .unwrap()
            .0
            .contains("SOURCE CHANGED")
    );
    assert!(screen(&mut app, 100, 30).0.contains("SOURCE CHANGED"));
    press(&mut app, KeyCode::Char('p'));
    assert!(
        app.document
            .notice
            .as_ref()
            .unwrap()
            .0
            .contains("SOURCE CHANGED")
    );
}
#[test]
fn diff_jumps_to_symbols_and_evidence_opens_the_cited_file() {
    let target = Target {
        file: "src/cache.rs".into(),
        symbol: Some("refresh".into()),
        line: 40,
    };
    let patch = json!({"changes":[{"file":"src/cache.rs","symbols":[{"start_line":40,"end_line":65}],"diff":"--- a/src/cache.rs\n+++ b/src/cache.rs\n@@ -1,2 +1,2 @@\n-old\n+new\n@@ -39,3 +39,3 @@\n-old\n+refresh"}]});
    // File headers are not shown, so the second hunk starts on line 3.
    assert_eq!(document::diff(&patch, target).scroll, 3);
    let doc = document::evidence(artifact(Some("README.md")), 0).unwrap();
    assert_eq!(doc.target.unwrap().file, "src/cache.rs");
    assert_eq!(
        document::artifact_target(doc.artifact.as_ref().unwrap())
            .unwrap()
            .file,
        "README.md"
    );
}
#[test]
fn review_marks_are_retained_only_for_unchanged_files() {
    let mut app = workspace();
    select(&mut app, "src/cache.rs");
    press(&mut app, KeyCode::Char('m'));
    select(&mut app, "src/http/client.rs");
    press(&mut app, KeyCode::Char('m'));
    let mut new = app.review.clone();
    new["file_hashes"]["src/cache.rs"] = json!("new content");
    app.explorer.refresh(&app.review, &new);
    assert!(!app.explorer.reviewed.contains("src/cache.rs"));
    assert!(app.explorer.reviewed.contains("src/http/client.rs"));
    new["changes"] = json!([]);
    app.explorer.refresh(&app.review, &new);
    assert!(app.explorer.reviewed.is_empty());
    assert!(app.explorer.target().is_none());
}
#[test]
fn explanation_evidence_remains_associated_with_its_own_history_entry() {
    let mut app = workspace();
    app.open(document::explanation(artifact(Some("src/cache.rs"))));
    let mut other = (*artifact(Some("README.md"))).clone();
    other["packet"]["evidence"][0]["text"] = json!("second explanation's evidence");
    app.open(document::explanation(Arc::new(other)));
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Char('1'));
    let content = app
        .document
        .lines
        .iter()
        .map(|l| l.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(content.contains("fn get()"));
    assert!(!content.contains("second explanation"));
    assert!(app.open_evidence(99).is_err());
    app.open(document::help());
    assert!(app.open_evidence(0).is_err());
}
#[test]
fn compact_layout_keeps_both_panes_accessible_and_input_cursor_visible() {
    let mut app = workspace();
    for (width, height) in [(140, 44), (100, 30), (80, 24), (40, 12)] {
        app.focus = Focus::Files;
        let (files, _) = screen(&mut app, width, height);
        assert!(files.contains("cache.rs"));
        assert!(app.areas.files.width > 0);
        press(&mut app, KeyCode::Tab);
        let (reader, _) = screen(&mut app, width, height);
        assert!(reader.contains("Why") && reader.contains("Code"));
        assert!(app.areas.reader.width >= if width < 88 { width - 4 } else { 40 });
        press(&mut app, KeyCode::Char('/'));
        app.input = "/ask 这个函数为什么这样实现？ this is a long question about failures".into();
        let (_, mut terminal) = screen(&mut app, width, height);
        let cursor = terminal.backend_mut().get_cursor_position().unwrap();
        assert!(cursor.x < width);
        assert!(cursor.y < height);
        press(&mut app, KeyCode::Esc);
    }
    assert!(screen(&mut app, 24, 8).0.contains("enlarge terminal"));
}
#[test]
fn mouse_selects_files_and_scrolls_the_pane_under_the_pointer() {
    let mut app = workspace();
    screen(&mut app, 120, 32);
    let index = app
        .explorer
        .rows
        .iter()
        .position(|r| r.key == "src/cache.rs")
        .unwrap();
    app.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: app.areas.files.x + 5,
        row: app.areas.files.y + index as u16,
        modifiers: KeyModifiers::NONE,
    })
    .unwrap();
    assert_eq!(app.document.target.as_ref().unwrap().file, "src/cache.rs");
    assert_eq!(app.document.kind, View::Recorded);
    app.open(document::help());
    screen(&mut app, 120, 24);
    app.mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: app.areas.reader.x + 2,
        row: app.areas.reader.y + 2,
        modifiers: KeyModifiers::NONE,
    })
    .unwrap();
    assert_eq!(app.document.scroll, 3);
}
#[test]
fn command_input_changes_settings_without_starting_requests() {
    let mut app = workspace();
    press(&mut app, KeyCode::Char('/'));
    for c in "agent claude".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.agent, "claude");
    assert!(app.job.is_none());
    assert!(app.key(KeyCode::Char('q'), KeyModifiers::CONTROL).unwrap());
}
#[test]
fn cancellation_and_disconnected_workers_return_control() {
    let mut app = workspace();
    let (sender, receiver) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let observed = cancel.clone();
    app.job = Some(Job {
        receiver,
        cancel,
        started: Instant::now(),
        scope: "all changes".into(),
        key: "all changes|".into(),
        file: None,
        progress: "Working".into(),
        handle: thread::spawn(move || {
            while !observed.load(Ordering::Relaxed) {
                thread::yield_now();
            }
            sender
                .send(Update::Done(Err(anyhow::anyhow!("Cancelled"))))
                .unwrap();
        }),
    });
    press(&mut app, KeyCode::Esc);
    assert!(!app.job.as_ref().unwrap().cancel.load(Ordering::Relaxed));
    press(&mut app, KeyCode::Char('x'));
    for _ in 0..100 {
        app.poll();
        if app.job.is_none() {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    assert!(app.job.is_none());
    assert_eq!(app.status, "all changes: Cancelled");
    let (sender, receiver) = mpsc::channel();
    drop(sender);
    app.job = Some(Job {
        receiver,
        cancel: Arc::new(AtomicBool::new(false)),
        started: Instant::now(),
        scope: "all".into(),
        key: "all changes|".into(),
        file: None,
        progress: "Working".into(),
        handle: thread::spawn(|| {}),
    });
    app.poll();
    assert!(app.job.is_none());
    assert!(app.status.contains("stopped unexpectedly"));
}
#[test]
fn renders_review_diff_explanation_and_empty_states() {
    let mut app = workspace();
    let (text, terminal) = screen(&mut app, 140, 44);
    assert!(text.contains("e explain"));
    assert!(text.contains("Why") && text.contains("Code"));
    assert!(!text.contains("Understand the work"));
    assert!(!text.contains("Overview"));
    assert!(!text.contains("Choices"));
    assert!(!text.contains("Cache.refresh"));
    preview("start", &terminal);
    select(&mut app, "src/cache.rs");
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Enter);
    let (text, terminal) = screen(&mut app, 140, 44);
    assert!(text.contains("cache.get(key)"));
    assert!(
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .any(|c| c.fg == GREEN && c.bg == ADD_BG)
    );
    preview("diff", &terminal);
    app.open(document::explanation(artifact(Some("src/cache.rs"))));
    let (text, terminal) = screen(&mut app, 140, 44);
    assert!(text.contains("Inferred:"));
    assert!(text.contains("No agent conversation was available"));
    preview("explanation", &terminal);
    app.open_evidence(0).unwrap();
    preview("evidence", &screen(&mut app, 100, 30).1);
    app.focus = Focus::Files;
    preview("compact-files", &screen(&mut app, 70, 24).1);
    app.focus = Focus::Reader;
    preview("compact-reader", &screen(&mut app, 70, 24).1);
    let mut empty = Workspace::from_review(
        Path::new("/empty"),
        json!({"changes":[],"decisions":[],"sessions":[],"warnings":[]}),
    );
    assert!(screen(&mut empty, 100, 30).0.contains("No changed files"));
    assert!(empty.target().is_none());
    assert!(empty.job.is_none());
    if std::env::var_os("WY_TUI_PREVIEW_LIVE").is_some() {
        let root = std::env::current_dir().unwrap();
        let mut app = Workspace::new(&root).unwrap();
        select(&mut app, "src/tui.rs");
        app.preview_selection();
        app.show_notes();
        preview("live-agent-notes", &screen(&mut app, 160, 44).1);
        preview("live-agent-notes-110", &screen(&mut app, 110, 32).1);
        preview("live-agent-notes-80", &screen(&mut app, 80, 24).1);
    }
    if let Ok(path) = std::env::var("WY_TUI_PREVIEW_ARTIFACT") {
        let artifact: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let root = std::env::current_dir().unwrap();
        let review = crate::service::load(&root).unwrap();
        let mut app = Workspace::from_review(&root, review);
        app.open(document::explanation(Arc::new(artifact)));
        preview("self-review", &screen(&mut app, 140, 44).1);
        app.open_evidence(0).unwrap();
        preview("self-evidence", &screen(&mut app, 100, 30).1);
    }
}
// Optional render artifacts for visual QA, from the same terminal buffer users see.
#[test]
fn session_code_appears_without_a_git_diff_and_keeps_a_distinct_answer() {
    let mut review = review();
    let record = json!({"id":"recorded-edit","file":"src/recent.rs","agent":"codex","session_id":"coding-session","session_key":"saved-key","event_id":"event-4","timestamp":"2026-10-09T08:00:00Z","state":"recorded","format":"code","text":"fn generated() { return 42; }","truncated":false});
    review["changes"] = json!([]);
    review["recent_code"] = json!([record]);
    let mut app = Workspace::from_review(Path::new("/example/payments"), review);
    assert_eq!(app.document.kind, View::Recorded);
    assert_eq!(app.code.as_ref().unwrap().kind, View::SessionCode);
    let options = app.why_options(false).unwrap();
    assert_eq!(options.file.as_deref(), Some("src/recent.rs"));
    assert!(options.target.is_none());
    assert_eq!(
        options.session_edit.as_ref().unwrap()["id"],
        "recorded-edit"
    );
    // Why is the default view; the recorded code is one Tab away.
    app.focus = Focus::Code;
    let (text, terminal) = screen(&mut app, 140, 38);
    assert!(text.contains("· recorded"));
    assert!(text.contains("fn generated()"));
    assert!(text.contains("execution not confirmed"));
    preview("session-code", &terminal);
    let mut answer = (*artifact(Some("src/recent.rs"))).clone();
    answer["packet"]["focus_session_edit"] = crate::history::edit_ref(&record);
    app.open(document::explanation(Arc::new(answer)));
    assert_eq!(app.code.as_ref().unwrap().kind, View::SessionCode);
    press(&mut app, KeyCode::Char('i'));
    assert_eq!(
        app.question_options("Why?").session_edit.unwrap()["id"],
        "recorded-edit"
    );
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Char('d'));
    press(&mut app, KeyCode::Char('w'));
    assert!(app.job.is_none());
    assert_eq!(app.document.kind, View::Explanation);

    // A current diff and a historical session edit of the same file have separate answers.
    app.review["changes"] =
        json!([{"file":"src/recent.rs","diff":"+current code","symbols":[],"added_lines":[1]}]);
    app.explorer.rebuild(&app.review);
    let mut current = (*artifact(Some("src/recent.rs"))).clone();
    current["id"] = json!("current-answer");
    app.open(document::explanation(Arc::new(current)));
    press(&mut app, KeyCode::Char('d'));
    press(&mut app, KeyCode::Char('w'));
    assert_eq!(
        app.document.artifact.as_ref().unwrap()["id"],
        "current-answer"
    );
    // The legacy c shortcut also shows the unified current changes.
    press(&mut app, KeyCode::Char('c'));
    assert_eq!(app.code.as_ref().unwrap().kind, View::Diff);
    app.change_view(View::SessionCode).unwrap();
    press(&mut app, KeyCode::Char('w'));
    assert_eq!(
        app.document.artifact.as_ref().unwrap()["packet"]["focus_session_edit"]["id"],
        "recorded-edit"
    );
    assert!(app.job.is_none());
}

#[test]
fn explanation_and_changes_share_one_reader_and_sources_open_by_mouse_and_keyboard() {
    let mut app = workspace();
    select(&mut app, "src/cache.rs");
    press(&mut app, KeyCode::Enter);
    app.open(document::explanation(artifact(Some("src/cache.rs"))));
    let (text, terminal) = screen(&mut app, 140, 38);
    assert_eq!(app.areas.code.width, 0);
    assert!(text.contains("Repeated requests can reuse"));
    assert!(text.contains("Code") && text.contains("Enriched"));
    preview("code-and-explanation", &terminal);
    // Tab switches the reader between Changes and the open answer.
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.focus, Focus::Code);
    let (text, _) = screen(&mut app, 140, 38);
    assert!(text.contains("cache.get(key)"));
    assert_eq!(app.areas.reader.width, 0);
    press(&mut app, KeyCode::BackTab);
    assert_eq!(app.focus, Focus::Files);
    // From the tree, Tab returns to whichever view was shown last.
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.focus, Focus::Code);
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.focus, Focus::Reader);
    press(&mut app, KeyCode::Char('s'));
    screen(&mut app, 140, 38);
    let (source, _) = app.areas.sources[0];
    app.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: source.x + 1,
        row: source.y,
        modifiers: KeyModifiers::NONE,
    })
    .unwrap();
    assert_eq!(app.document.kind, View::Evidence);
    assert!(app.code.is_some());
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.document.kind, View::Explanation);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.document.kind, View::Evidence);

    // Narrow terminals keep both documents accessible as full-width readers.
    app.focus = Focus::Code;
    let (text, _) = screen(&mut app, 70, 24);
    assert!(text.contains("cache.get(key)"));
    press(&mut app, KeyCode::Tab);
    let (text, _) = screen(&mut app, 70, 24);
    assert!(text.contains("CAPTURED CODE"));
}

#[test]
fn source_navigation_reaches_references_beyond_nine_and_code_scrolls_independently() {
    let mut app = workspace();
    let mut answer = (*artifact(Some("src/cache.rs"))).clone();
    answer["packet"]["evidence"] = json!((0..12).map(|i| json!({"id":format!("code-{i}"),"kind":"code","file":format!("src/evidence_{i}.rs"),"start_line":1,"text":format!("source number {i}")})).collect::<Vec<_>>());
    answer["explanation"]["answer"]["evidence_ids"] =
        json!((0..12).map(|i| format!("code-{i}")).collect::<Vec<_>>());
    app.open(document::explanation(Arc::new(answer)));
    app.code.as_mut().unwrap().lines = (0..100).map(|i| Line::raw(format!("line {i}"))).collect();
    app.focus = Focus::Code;
    screen(&mut app, 120, 24);
    press(&mut app, KeyCode::PageDown);
    assert!(app.code.as_ref().unwrap().scroll > 0);
    assert_eq!(app.document.scroll, 0);
    press(&mut app, KeyCode::Char('s'));
    for _ in 0..11 {
        press(&mut app, KeyCode::Down);
    }
    screen(&mut app, 120, 24);
    assert!(app.areas.sources.iter().any(|(_, link)| *link == Link::Source(11)));
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.document.title, "[12] src/evidence_11.rs");
}

fn with_notes() -> Workspace {
    let mut app = workspace();
    app.review["sessions"] = json!([{"id":"coding","agent":"codex","storage_key":"saved-session"}]);
    app.review["recent_code"] =
        json!([{"file":"src/cache.rs","event_id":"edit","session_key":"saved-session"}]);
    app.sessions = vec![
        json!({"id":"coding","agent":"codex","path":"session.jsonl","events":[
            {"id":"request","kind":"user","text":"Avoid fetching the same response repeatedly.","timestamp":"2026-10-09T08:00:00Z"},
            {"id":"reason","kind":"assistant","text":"I’ll keep responses in memory because repeated reads can reuse a response without another network call.","timestamp":"2026-10-09T08:01:00Z"},
            {"id":"edit","kind":"change","files":["src/cache.rs"],"text":"patch","timestamp":"2026-10-09T08:02:00Z"}
        ]}),
    ];
    for event in app.sessions[0]["events"].as_array_mut().unwrap() {
        event["provenance"] = json!({"source_type":if event["kind"]=="change"{"tool_record"}else{"original_turn"},"basis":"test_native_event","original_refs":[]});
    }
    select(&mut app, "src/cache.rs");
    app.preview_selection();
    app.show_notes();
    app.focus = Focus::Files;
    app
}
fn mock_job(app: &mut Workspace, file: &str) -> mpsc::Sender<Update> {
    let (sender, receiver) = mpsc::channel();
    app.job = Some(Job {
        receiver,
        cancel: Arc::new(AtomicBool::new(false)),
        handle: thread::spawn(|| {}),
        started: Instant::now(),
        scope: file.into(),
        key: format!("{file}|"),
        file: Some(file.into()),
        progress: "Reading notes".into(),
    });
    sender
}
#[test]
fn automatic_notes_are_offline_cited_and_offer_enrichment() {
    let mut app = with_notes();
    let (text, terminal) = screen(&mut app, 140, 38);
    assert!(text.contains("because"));
    assert!(
        app.document
            .lines
            .iter()
            .any(|l| l.to_string().contains("because repeated reads"))
    );
    assert!(!text.contains("alternatives and tradeoffs"));
    assert!(!text.contains("What made this approach"));
    assert!(text.contains("e explain"));
    assert!(!text.contains("Session code"));
    assert!(!text.contains("Why this change?"));
    // One reader: Notes replaces Changes instead of sitting beside it.
    assert!(app.areas.reader.width > 0);
    assert_eq!(app.areas.code.width, 0);
    assert!(app.job.is_none());
    let options = app.why_options(false).unwrap();
    assert_eq!(options.note_refs.len(), 2);
    assert!(!options.question.contains("Notes about the captured excerpts"));
    preview("automatic-notes", &terminal);
    press(&mut app, KeyCode::Char('2'));
    assert_eq!(app.document.kind, View::Evidence);
    assert!(
        app.document
            .lines
            .iter()
            .any(|l| l.to_string().contains("because repeated reads"))
    );
    assert_eq!(app.why_options(false).unwrap().note_refs, options.note_refs);
    assert_eq!(
        app.question_options("What is missing?").note_refs,
        options.note_refs
    );
    assert!(
        app.question_options("What is missing?")
            .question
            .find("Previous assessment")
            .is_none()
    );
    assert!(app.job.is_none());
}
#[test]
fn background_completion_stays_with_its_file_and_preserves_reading_positions() {
    let mut app = with_notes();
    app.code.as_mut().unwrap().scroll = 3;
    let sender = mock_job(&mut app, "src/cache.rs");
    select(&mut app, "README.md");
    app.preview_selection();
    app.document.scroll = 2;
    assert_eq!(app.file_state("src/cache.rs").unwrap().0, "working");
    sender
        .send(Update::Done(Ok((*artifact(Some("src/cache.rs"))).clone())))
        .unwrap();
    app.poll();
    assert!(app.job.is_none());
    assert_eq!(app.document.target.as_ref().unwrap().file, "README.md");
    assert_eq!(app.document.scroll, 2);
    assert_eq!(app.focus, Focus::Files);
    assert_eq!(app.file_state("src/cache.rs").unwrap().0, "ready");
    select(&mut app, "src/cache.rs");
    app.preview_selection();
    assert_eq!(app.document.kind, View::Explanation);
    assert_eq!(app.code.as_ref().unwrap().scroll, 3);
    app.document.scroll = 4;
    select(&mut app, "README.md");
    app.preview_selection();
    assert_eq!(app.document.scroll, 2);
    select(&mut app, "src/cache.rs");
    app.preview_selection();
    assert_eq!(app.document.scroll, 4);
    assert!(app.job.is_none());
    preview("background-ready", &screen(&mut app, 140, 38).1);
}
#[test]
fn completion_does_not_interrupt_drafts_or_sources_and_ready_button_reuses_answer() {
    let mut app = with_notes();
    press(&mut app, KeyCode::Char('i'));
    app.input = "How is expiry handled?".into();
    app.finish_answer("src/cache.rs|", artifact(Some("src/cache.rs")));
    assert_eq!(app.document.kind, View::Recorded);
    assert_eq!(app.input, "How is expiry handled?");
    assert_eq!(app.question_scope(), "src/cache.rs");
    press(&mut app, KeyCode::Esc);
    select(&mut app, "README.md");
    app.preview_selection();
    select(&mut app, "src/cache.rs");
    app.preview_selection();
    assert_eq!(app.document.kind, View::Explanation);
    press(&mut app, KeyCode::Char('o'));
    press(&mut app, KeyCode::Char('2'));
    app.finish_answer("src/cache.rs|", artifact(Some("src/cache.rs")));
    assert_eq!(app.document.kind, View::Evidence);
    press(&mut app, KeyCode::Char('o'));
    let (text, _) = screen(&mut app, 140, 38);
    assert!(text.contains("Enriched"));
    let button = app
        .areas
        .tabs
        .iter()
        .find(|(_, view, _)| *view == View::Explanation)
        .unwrap()
        .0;
    app.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: button.x + 1,
        row: button.y,
        modifiers: KeyModifiers::NONE,
    })
    .unwrap();
    assert_eq!(app.document.kind, View::Explanation);
    assert!(app.job.is_none());
}
#[test]
fn requests_queue_once_per_scope_and_cancel_clears_the_queue() {
    let mut app = with_notes();
    let sender = mock_job(&mut app, "src/cache.rs");
    app.start(app.why_options(false).unwrap());
    assert!(app.queue.is_empty());
    select(&mut app, "README.md");
    app.preview_selection();
    let options = app.why_options(false).unwrap();
    app.start(options.clone());
    app.start(options);
    assert_eq!(app.queue.len(), 1);
    assert_eq!(app.file_state("README.md").unwrap().0, "queued");
    let (text, terminal) = screen(&mut app, 140, 38);
    assert!(text.contains("README.md · queued"));
    assert!(text.contains("1 queued"));
    preview("background-queued", &terminal);
    press(&mut app, KeyCode::Esc);
    assert!(!app.job.as_ref().unwrap().cancel.load(Ordering::Relaxed));
    press(&mut app, KeyCode::Char('x'));
    assert!(app.queue.is_empty());
    assert!(app.job.as_ref().unwrap().cancel.load(Ordering::Relaxed));
    sender
        .send(Update::Done(Err(anyhow::anyhow!("Cancelled"))))
        .unwrap();
    app.poll();
    assert!(app.job.is_none());
    assert_eq!(app.document.target.as_ref().unwrap().file, "README.md");
    assert!(app.status.starts_with("src/cache.rs:"));
}
#[test]
fn restarting_restores_completed_enrichments_without_a_request() {
    let dir = tempfile::tempdir().unwrap();
    crate::repository::git(dir.path(), &["init", "-q"], true).unwrap();
    std::fs::write(dir.path().join("lib.rs"), "fn example() {}\n").unwrap();
    let store = crate::storage::Store::open(dir.path()).unwrap();
    let mut answer = (*artifact(Some("lib.rs"))).clone();
    store.put("reasoning", "earlier", &answer).unwrap();
    answer["id"] = json!("newest");
    store.put("reasoning", "newest", &answer).unwrap();
    store.put("reasoning", "latest", &answer).unwrap();
    assert_eq!(store.recent("reasoning", 40).unwrap().len(), 2);
    let app = Workspace::new(dir.path()).unwrap();
    assert_eq!(app.document.kind, View::Explanation);
    assert_eq!(app.document.artifact.unwrap()["id"], "newest");
    assert!(app.job.is_none());
    assert_eq!(app.answers.len(), 1);
}

fn commit_workspace() -> (tempfile::TempDir, Workspace, String, String) {
    commit_workspace_with_rows(&[])
}
fn commit_workspace_with_rows(extra: &[Value]) -> (tempfile::TempDir, Workspace, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.name", "Test"],
        vec!["config", "user.email", "test@example.invalid"],
    ] {
        crate::repository::git(root, &args, true).unwrap();
    }
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> i32 { 1 }\n").unwrap();
    crate::repository::git(root, &["add", "lib.rs"], true).unwrap();
    crate::repository::git(root, &["commit", "-qm", "Initial answer"], true).unwrap();
    let base = crate::repository::head(root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();
    let history = root.join(".codex/sessions");
    std::fs::create_dir_all(&history).unwrap();
    let path = history.join("coding.jsonl");
    let mut rows = vec![
        json!({"type":"session_meta","payload":{"id":"coding","cwd":root.canonicalize().unwrap()}}),
        json!({"type":"turn_context","payload":{"turn_id":"original-turn"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","id":"original-message","content":[{"type":"output_text","text":"Return 42 because it is the agreed API value."}]}}),
    ];
    rows.extend_from_slice(extra);
    std::fs::write(
        &path,
        rows.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let review = service::review(
        root,
        &service::ReviewOptions {
            source: "codex".into(),
            ..Default::default()
        },
    )
    .unwrap();
    crate::repository::git(root, &["add", "lib.rs"], true).unwrap();
    crate::repository::git(root, &["commit", "-qm", "Return agreed answer"], true).unwrap();
    let hash = crate::repository::head(root).unwrap();
    std::fs::remove_file(path).unwrap();
    let app = Workspace::from_review(root, review);
    (dir, app, base, hash)
}
fn run_command(app: &mut Workspace, command: &str) -> Result<()> {
    app.edit(Input::Command);
    app.input = command.into();
    app.command()
}
fn mouse_at(app: &mut Workspace, kind: MouseEventKind, x: u16, y: u16) {
    app.mouse(MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    })
    .unwrap();
}

#[test]
fn interactive_commit_lookup_browses_saved_history_and_restores_previous_view() {
    let (_dir, mut app, base, hash) = commit_workspace();
    let selected = app.explorer.target();
    let original = app.document.kind;
    press(&mut app, KeyCode::Char('g'));
    assert_eq!(app.document.kind, View::Commits);
    assert_eq!(app.document.commits, [hash.clone(), base]);
    assert_eq!(app.document.source_selection, Some(0));
    let (text, terminal) = screen(&mut app, 130, 30);
    assert!(text.contains("Return agreed answer"));
    preview("commit-picker", &terminal);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.document.kind, View::Commit);
    let (text, terminal) = screen(&mut app, 130, 30);
    assert!(text.contains("agreed API value"));
    assert!(text.contains("Matched review base and source snapshot"));
    preview("commit-conversation", &terminal);
    assert_eq!(app.explorer.target(), selected);
    assert!(app.code.is_none());
    assert!(app.target().is_none());
    press(&mut app, KeyCode::Char('e'));
    assert!(app.job.is_none());
    assert!(run_command(&mut app, "/ask Why?").is_err());
    assert!(app.job.is_none());
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.document.kind, View::Commits);
    assert_eq!(app.document.source_selection, Some(0));
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.document.kind, original);
    assert!(app.code.is_some());
    run_command(&mut app, &format!("/commit {}", &hash[..8])).unwrap();
    assert_eq!(app.document.kind, View::Commit);
    assert!(app.job.is_none());
}

#[test]
fn interactive_commit_mouse_navigation_missing_history_and_explicit_linking() {
    let (_dir, mut app, base, _) = commit_workspace();
    screen(&mut app, 130, 30);
    press(&mut app, KeyCode::Char('g'));
    assert_eq!(app.document.kind, View::Commits);
    press(&mut app, KeyCode::End);
    assert_eq!(app.document.source_selection, Some(1));
    assert!(app.key(KeyCode::Enter, KeyModifiers::NONE).is_err());
    assert_eq!(app.document.kind, View::Commits);
    assert!(run_command(&mut app, "/commit missing-revision").is_err());
    assert_eq!(app.document.kind, View::Commits);
    run_command(&mut app, &format!("/link {base}")).unwrap();
    assert_eq!(app.document.kind, View::Commit);
    assert!(
        screen(&mut app, 130, 30)
            .0
            .contains("Explicitly linked review")
    );
    run_command(&mut app, "/commits").unwrap();
    screen(&mut app, 130, 30);
    let source = app.areas.sources[0].0;
    mouse_at(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        source.x + 1,
        source.y,
    );
    assert_eq!(app.document.kind, View::Commit);
    assert!(app.job.is_none());
    assert!(app.queue.is_empty());
}

#[test]
fn compacted_commit_sources_open_pinned_originals_and_show_missing_turns() {
    let extra = [
        json!({"type":"compacted","payload":{"message":"The summary reports the API value changed.","source_refs":[{"turn_id":"original-turn","message_id":"original-message"}]}}),
        json!({"type":"compacted","payload":{"message":"Earlier discussion is no longer captured."}}),
    ];
    let (_dir, mut app, _, hash) = commit_workspace_with_rows(&extra);
    run_command(&mut app, &format!("/commit {hash}")).unwrap();
    let (text, terminal) = screen(&mut app, 140, 42);
    assert!(text.contains("Secondary evidence · Compacted summary"));
    assert!(text.contains("Original turn unavailable"));
    assert_eq!(app.document.originals.len(), 1);
    preview("commit-provenance", &terminal);
    press(&mut app, KeyCode::Char('s'));
    assert!(screen(&mut app, 140, 42).0.contains("Enter open"));
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.document.kind, View::Original);
    let (text, terminal) = screen(&mut app, 140, 32);
    assert!(text.contains("agreed API value"));
    assert!(text.contains("original-message"));
    assert!(!text.contains("e Enrich"));
    preview("original-turn", &terminal);
    press(&mut app, KeyCode::Char('e'));
    assert!(app.job.is_none());
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.document.kind, View::Commit);
    screen(&mut app, 140, 42);
    let source = app.areas.sources[0].0;
    mouse_at(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        source.x + 1,
        source.y,
    );
    assert_eq!(app.document.kind, View::Original);
    assert!(app.job.is_none());
}

#[test]
fn explanation_summary_sources_preserve_back_navigation_and_legacy_warning() {
    let extra = [
        json!({"type":"compacted","payload":{"message":"Summary of the API change.","source_refs":[{"message_id":"original-message"}]}}),
    ];
    let (_dir, mut app, _, hash) = commit_workspace_with_rows(&extra);
    let context = crate::commits::lookup(&app.root, &hash, "both").unwrap();
    let session = &context["sessions"][0];
    let mut summary = crate::history::event_evidence(session, &session["events"][1]);
    crate::history::origins::enrich(&app.root, &mut summary).unwrap();
    let mut answer = (*artifact(Some("lib.rs"))).clone();
    answer["packet"]["evidence"] = json!([summary]);
    answer["explanation"]["judgments"][0]["evidence_ids"] = json!([summary["id"]]);
    answer["explanation"]["judgments"][0]["status"] = json!("recorded");
    answer["explanation"]["judgments"][0]["quote_id"] = summary["id"].clone();
    answer["explanation"]["judgments"][0]["quote"] = summary["text"].clone();
    let answer = Arc::new(answer);
    app.open(document::explanation(answer.clone()));
    assert!(screen(&mut app, 150, 42).0.contains("Earlier assessment"));
    app.open(document::evidence(answer, 0).unwrap());
    let (text, terminal) = screen(&mut app, 150, 42);
    assert!(text.contains("Secondary evidence"));
    assert!(text.contains("Original turns available"));
    preview("summary-evidence", &terminal);
    press(&mut app, KeyCode::Char('s'));
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.document.kind, View::Original);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.document.kind, View::Evidence);
    assert_eq!(app.document.originals.len(), 1);
}

#[test]
fn commit_picker_handles_empty_repositories_and_long_lists_on_small_screens() {
    let dir = tempfile::tempdir().unwrap();
    crate::repository::git(dir.path(), &["init", "-q"], true).unwrap();
    let mut app = Workspace::from_review(dir.path(), json!({"changes":[]}));
    press(&mut app, KeyCode::Char('g'));
    assert!(screen(&mut app, 80, 24).0.contains("No commits yet"));
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.document.kind, View::Commits);
    let entries: Vec<_> = (0..50).map(|i| json!({"commit":format!("commit-{i}"),"short":format!("hash-{i}"),"subject":format!("A deliberately long commit subject for change {i} which wraps in a narrow terminal")})).collect();
    app.open(document::commits(&entries));
    press(&mut app, KeyCode::End);
    let (_, _) = screen(&mut app, 40, 16);
    assert_eq!(app.document.source_selection, Some(49));
    assert!(app.document.scroll > 0);
    assert!(app.areas.sources.iter().any(|(_, link)| *link == Link::Source(49)));
    press(&mut app, KeyCode::Home);
    screen(&mut app, 40, 16);
    assert_eq!(app.document.scroll, 0);
    assert!(app.areas.sources.iter().any(|(_, link)| *link == Link::Source(0)));
}

#[test]
fn pane_dividers_drag_resize_and_persist_without_changing_reading_context() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = workspace();
    app.root = dir.path().into();
    select(&mut app, "src/cache.rs");
    app.preview_selection();
    screen(&mut app, 160, 36);
    let selected = app.explorer.target();
    let original_kind = app.document.kind;
    let divider = app.areas.file_divider;
    mouse_at(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        divider.x,
        divider.y + 3,
    );
    mouse_at(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        divider.x + 10,
        divider.y + 3,
    );
    screen(&mut app, 160, 36);
    assert_eq!(app.areas.file_divider.x, divider.x + 10);
    mouse_at(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        divider.x + 10,
        divider.y + 3,
    );
    let (_, terminal) = screen(&mut app, 160, 36);
    assert!(app.areas.reader.width >= 40);
    assert_eq!(app.explorer.target(), selected);
    assert_eq!(app.document.kind, original_kind);
    assert!(app.dragging.is_none());
    assert!(app.job.is_none());
    preview("adjusted-panes", &terminal);
    let restored = PaneSizes::load(dir.path()).unwrap();
    assert_eq!(restored.files, app.pane_sizes.files);
    // Terminal size changes clamp displayed widths without destroying preferences.
    app.focus = Focus::Reader;
    screen(&mut app, 40, 16);
    assert_eq!(app.areas.file_divider.width, 0);
    screen(&mut app, 110, 28);
    assert!(app.areas.reader.width >= 40);
    screen(&mut app, 160, 36);
    assert_eq!(app.pane_sizes.files, restored.files);
}

#[test]
fn keyboard_resizes_active_dividers_preserves_typing_and_can_reset() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = workspace();
    app.root = dir.path().into();
    screen(&mut app, 150, 32);
    let files = app.areas.file_divider.x;
    press(&mut app, KeyCode::Char(']'));
    screen(&mut app, 150, 32);
    assert_eq!(app.areas.file_divider.x, files + 3);
    let width = app.pane_sizes.files;
    press(&mut app, KeyCode::Char('/'));
    press(&mut app, KeyCode::Char('['));
    assert_eq!(app.input, "/[");
    assert_eq!(app.pane_sizes.files, width);
    press(&mut app, KeyCode::Esc);
    run_command(&mut app, "/layout reset").unwrap();
    assert_eq!(app.pane_sizes.files, None);
    assert_eq!(PaneSizes::load(dir.path()).unwrap().files, None);
    app.sidebar = false;
    screen(&mut app, 80, 24);
    press(&mut app, KeyCode::Char(']'));
    assert_eq!(app.pane_sizes.files, None);
    assert!(app.status.contains("Widen"));
}

fn preview(name: &str, terminal: &Terminal<TestBackend>) {
    let Some(path) = std::env::var_os("WY_TUI_PREVIEW_DIR") else {
        return;
    };
    let buffer = terminal.backend().buffer();
    let color = |c: Color| match c {
        Color::Rgb(r, g, b) => json!([r, g, b]),
        _ => Value::Null,
    };
    let cells = buffer.content.iter().map(|c| json!({"text":c.symbol(),"fg":color(c.fg),"bg":color(c.bg),"bold":c.modifier.contains(Modifier::BOLD)})).collect::<Vec<_>>();
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(
        Path::new(&path).join(format!("{name}.json")),
        json!({"width":buffer.area.width,"height":buffer.area.height,"cells":cells}).to_string(),
    )
    .unwrap();
}
