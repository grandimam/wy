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
    assert_eq!(app.focus, Focus::Reader);
    assert_eq!(app.document.kind, View::Diff);
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
    assert_eq!(app.document.kind, View::Diff);
    assert_eq!(app.document.target, app.explorer.target());
    press(&mut app, KeyCode::Down);
    assert_eq!(app.document.target.as_ref().unwrap().file, "src/cache.rs");
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Right);
    assert_eq!(app.document.target.as_ref().unwrap().line, 3);
    assert!(app.document.target.as_ref().unwrap().symbol.is_some());
    assert!(app.job.is_none());
    assert!(app.back.is_empty());
    for key in ['a', 'e', 'o'] {
        press(&mut app, KeyCode::Char(key));
    }
    assert!(app.job.is_none());
    assert_eq!(app.document.kind, View::Diff);
}
#[test]
fn why_requests_use_the_selected_change_and_refresh_the_original_scope() {
    let mut app = workspace();
    select(&mut app, "src/cache.rs");
    let options = app.why_options(false).unwrap();
    assert_eq!(options.target.as_deref(), Some("src/cache.rs"));
    assert_eq!(options.question, reasoning::prompt("change_reason"));
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
    press(&mut app, KeyCode::Char('d'));
    assert_eq!(app.document.kind, View::Diff);
    let (_, _) = screen(&mut app, 120, 32);
    let why = app
        .areas
        .tabs
        .iter()
        .find(|(_, view)| *view == View::Explanation)
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
    answer["packet"]["evidence"].as_array_mut().unwrap().push(json!({"id":"statement-1","kind":"session","file":"conversation.jsonl","agent":"codex","role":"assistant","start_line":9,"text":quote}));
    answer["explanation"]["judgments"][0] = json!({"choice":"Cache repeated reads","reason":"The agent explicitly connected caching to avoiding repeated fetches.","status":"recorded","quote":quote,"quote_id":"statement-1","evidence_ids":["statement-1","code-1"]});
    let answer = Arc::new(answer);
    let doc = document::explanation(answer.clone());
    let text = doc
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.find(quote).unwrap() < text.find("Answer").unwrap());
    assert!(text.contains("Recorded in the conversation [1] [2]"));
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
    assert_eq!(document::diff(&patch, target).scroll, 5);
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
    app.change_view(View::Diff).unwrap();
    assert!(app.open_evidence(0).is_err());
}
#[test]
fn compact_layout_keeps_both_panes_accessible_and_input_cursor_visible() {
    let mut app = workspace();
    for (width, height) in [(140, 44), (100, 30), (80, 24), (40, 12)] {
        app.focus = Focus::Files;
        let (files, _) = screen(&mut app, width, height);
        assert!(files.contains("Files"));
        press(&mut app, KeyCode::Tab);
        let (reader, _) = screen(&mut app, width, height);
        assert!(reader.contains("Diff"));
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
    assert_eq!(app.document.kind, View::Diff);
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
    for _ in 0..100 {
        app.poll();
        if app.job.is_none() {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    assert!(app.job.is_none());
    assert_eq!(app.status, "Cancelled");
    let (sender, receiver) = mpsc::channel();
    drop(sender);
    app.job = Some(Job {
        receiver,
        cancel: Arc::new(AtomicBool::new(false)),
        started: Instant::now(),
        scope: "all".into(),
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
    assert!(text.contains("Why this change?"));
    assert!(text.contains("Diff"));
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
    assert!(text.contains("Inferred · not an agent statement [1]"));
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
