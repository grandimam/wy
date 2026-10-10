use super::*;
use ratatui::backend::TestBackend;
use serde_json::json;

fn app() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    crate::repository::git(&root, &["init", "-q"], true).unwrap();
    std::fs::write(root.join("worker.rs"), "CURRENT_CODE_ONLY\n").unwrap();
    let path = root.join(".codex/sessions/session.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let rows = [
        json!({"type":"session_meta","payload":{"id":"coding","cwd":root,"timestamp":"2026-10-09T10:00:00Z"}}),
        json!({"type":"response_item","timestamp":"2026-10-09T10:01:00Z","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Persist background jobs"}]}}),
        json!({"type":"response_item","timestamp":"2026-10-09T10:02:00Z","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"WY_DECISION {\"file\":\"worker.rs\",\"decision\":\"Persist jobs in PostgreSQL\",\"reason\":\"Jobs must survive restarts\",\"alternatives\":[\"Redis\"],\"tradeoffs\":[\"Polling overhead\"]}"}]}}),
        json!({"type":"response_item","timestamp":"2026-10-09T10:03:00Z","payload":{"type":"custom_tool_call","name":"apply_patch","call_id":"c1","input":"*** Begin Patch\n*** Add File: worker.rs\n+persist(job)\n*** End Patch"}}),
        json!({"type":"response_item","timestamp":"2026-10-09T10:04:00Z","payload":{"type":"function_call_output","call_id":"c1","output":"Success. Updated the following files:"}}),
    ];
    std::fs::write(
        path,
        rows.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let review = service::review(
        &root,
        &service::ReviewOptions {
            source: "codex".into(),
        },
    )
    .unwrap();
    let sessions = crate::history::saved(&review).unwrap();
    let mut app = Workspace::from_review(&root, review);
    app.sessions = sessions;
    app.source = "codex".into();
    app.select_latest_work();
    app.decision_home();
    (dir, app)
}
fn text(doc: &Document) -> String {
    doc.lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn session_decisions_open_captured_code_and_current_code_only_on_request() {
    let (_dir, mut app) = app();
    assert_eq!(app.document.kind, View::DecisionOverview);
    assert_eq!(app.focus, Focus::Reader);
    assert!(!app.sidebar);
    assert!(text(&app.document).contains("Persist background jobs"));
    assert!(!text(&app.document).contains("Uncommitted changes"));
    assert!(!text(&app.document).contains("CURRENT_CODE_ONLY"));
    assert_eq!(
        app.document.artifact.as_ref().unwrap()["scope_kind"],
        "session"
    );
    app.key(KeyCode::Enter, KeyModifiers::NONE).unwrap();
    assert_eq!(app.document.kind, View::DecisionDetail);
    let detail = text(&app.document);
    assert!(detail.contains("Redis") && detail.contains("Polling overhead"));
    assert!(detail.find("Evidence").unwrap() < detail.find("Original context").unwrap());
    let index = app
        .document
        .sources
        .iter()
        .find_map(|(_, l)| {
            if let Link::Source(i) = l {
                Some(*i)
            } else {
                None
            }
        })
        .unwrap();
    app.follow(Link::Source(index)).unwrap();
    assert!(text(&app.document).contains("persist(job)"));
    assert!(!text(&app.document).contains("CURRENT_CODE_ONLY"));
    assert_eq!(app.section(), View::DecisionOverview);
    let comparison = app
        .document
        .sources
        .iter()
        .find_map(|(_, l)| {
            if let Link::CompareSessionEdit(_) = l {
                Some(l.clone())
            } else {
                None
            }
        })
        .unwrap();
    app.follow(comparison).unwrap();
    assert!(text(&app.document).contains("Current code · read now"));
    assert!(text(&app.document).contains("CURRENT_CODE_ONLY"));
    assert!(app.job.is_none());
    assert!(app.answers.is_empty());
}
#[test]
fn historical_evidence_shortcuts_never_queue_current_code_assessments() {
    let (_dir, mut app) = app();
    app.follow(Link::Decision(0)).unwrap();
    assert!(app.why_options(false).is_none());
    let source = app
        .document
        .sources
        .iter()
        .find_map(|(_, l)| {
            if let Link::Source(i) = l {
                Some(*i)
            } else {
                None
            }
        })
        .unwrap();
    app.follow(Link::Source(source)).unwrap();
    assert!(app.why_options(false).is_none());
    // A fake busy job prevents a regression from invoking a real installed CLI.
    let (_sender, receiver) = mpsc::channel();
    app.job = Some(Job {
        receiver,
        cancel: Arc::new(AtomicBool::new(false)),
        handle: thread::spawn(|| {}),
        started: Instant::now(),
        scope: "fixture".into(),
        key: "fixture".into(),
        file: None,
        progress: "fixture".into(),
    });
    app.key(KeyCode::Char('e'), KeyModifiers::NONE).unwrap();
    app.input = "/ask Why was this chosen?".into();
    app.editing = Some(Input::Command);
    assert!(app.command().is_err());
    assert!(app.queue.is_empty());
    app.key(KeyCode::Char('v'), KeyModifiers::NONE).unwrap();
    assert_eq!(app.document.kind, View::DecisionOverview);
}

#[test]
fn decision_completion_stays_with_selected_snapshot_and_does_not_interrupt_reading() {
    let (_dir, mut app) = app();
    let mut brief = (*app.document.artifact.clone().unwrap()).clone();
    brief["mode"] = json!("assessment");
    app.follow(Link::Decision(0)).unwrap();
    app.document.scroll = 7;
    app.finish_decisions(Arc::new(brief.clone()));
    assert_eq!(app.document.kind, View::DecisionDetail);
    assert_eq!(app.document.scroll, 7);
    app.decision_home();
    app.edit(Input::Command);
    let before = app.document.artifact.clone().unwrap();
    brief["id"] = json!("updated");
    app.finish_decisions(Arc::new(brief.clone()));
    assert_eq!(app.document.artifact.as_ref().unwrap()["id"], before["id"]);
    app.editing = None;
    app.draft = None;
    app.decision_home();
    assert_eq!(app.document.artifact.as_ref().unwrap()["id"], "updated");
    brief["scope_key"] = json!("other-session");
    brief["id"] = json!("wrong");
    app.finish_decisions(Arc::new(brief));
    assert_eq!(app.decision_brief.as_ref().unwrap()["id"], "updated");
}
#[test]
fn only_decisions_and_sessions_are_primary_and_picker_remains_available() {
    let (_dir, mut app) = app();
    for width in [40, 80, 140] {
        app.decision_home();
        let mut terminal = Terminal::new(TestBackend::new(width, 32)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        assert_eq!(
            app.areas
                .tabs
                .iter()
                .map(|(_, v, _)| *v)
                .collect::<Vec<_>>(),
            vec![View::DecisionOverview, View::Sessions]
        );
        assert!(app.areas.files.is_empty());
        if width >= 88 {
            assert!(
                app.areas
                    .tabs
                    .iter()
                    .all(|(r, _, _)| r.right() <= app.areas.reader.x)
            );
        }
        let rect = app.areas.tabs[1].0;
        app.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        })
        .unwrap();
        assert_eq!(app.document.kind, View::SessionWork);
        assert!(text(&app.document).contains("Your request"));
        assert!(!text(&app.document).contains("Implementation flow"));
        assert!(!text(&app.document).contains("CURRENT_CODE_ONLY"));
        terminal.draw(|f| app.draw(f)).unwrap();
        assert!(app.areas.files.is_empty());
        app.follow(Link::SessionPicker).unwrap();
        assert_eq!(app.document.kind, View::Sessions);
        assert!(text(&app.document).contains("latest captured"));
        let link = app.document.sources[0].1.clone();
        app.follow(link).unwrap();
        assert_eq!(app.document.kind, View::SessionWork);
        let code = app
            .document
            .sources
            .iter()
            .find_map(|(_, l)| {
                if let Link::SessionEdit(_) = l {
                    Some(l.clone())
                } else {
                    None
                }
            })
            .unwrap();
        app.follow(code).unwrap();
        assert_eq!(app.document.kind, View::SessionImplementation);
        assert!(text(&app.document).contains("persist(job)"));
        assert!(app.job.is_none());
    }
}
#[test]
fn session_navigation_supports_click_keyboard_scrolling_and_narrow_terminals() {
    let (_dir, mut app) = app();
    let base = app.sessions[0].clone();
    for index in 0..30 {
        let mut session = base.clone();
        session["id"] = json!(format!("older-{index}"));
        let key = crate::session_work::snapshot_key(&session);
        app.review["sessions"].as_array_mut().unwrap().push(json!({"storage_key":key,"id":session["id"],"agent":session["agent"]}));
        app.sessions.push(session);
    }
    app.rebuild_session_navigation();
    assert_eq!(app.navigation_sessions().len(), 31);
    assert_eq!(app.navigation_sessions()[0].1, "coding");
    // Navigation must use cached metadata, even without loaded transcripts.
    let entries = app.navigation_sessions().to_vec();
    app.sessions.clear();
    assert_eq!(app.navigation_sessions(), entries.as_slice());
    let mut terminal = Terminal::new(TestBackend::new(120, 18)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    let rect = app.areas.sessions;
    assert!(!rect.is_empty());
    assert!(rect.right() <= app.areas.reader.x);
    app.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: rect.x, row: rect.y, modifiers: KeyModifiers::NONE }).unwrap();
    assert_eq!(app.document.kind, View::SessionWork);
    assert!(!text(&app.document).contains("Open conversation"));
    assert!(!text(&app.document).contains("Original context"));
    assert!(!text(&app.document).contains("Capture details"));
    assert!(text(&app.document).contains("Tool activity"));
    app.key(KeyCode::Tab, KeyModifiers::NONE).unwrap();
    app.key(KeyCode::End, KeyModifiers::NONE).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    assert_eq!(app.session_nav.selected(), Some(9));
    assert!(app.session_nav.offset() > 0);
    let selection = app.selected_work.as_ref().unwrap()["session"]["id"].clone();
    let next = app.areas.sources.iter().find(|(_, link)| *link == Link::SessionPage(1)).unwrap().0;
    app.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: next.x, row: next.y, modifiers: KeyModifiers::NONE }).unwrap();
    assert_eq!(app.session_page, 1);
    assert_eq!(app.session_nav.selected(), Some(0));
    app.key(KeyCode::PageDown, KeyModifiers::NONE).unwrap();
    app.key(KeyCode::PageDown, KeyModifiers::NONE).unwrap();
    assert_eq!(app.session_page, 3);
    app.key(KeyCode::End, KeyModifiers::NONE).unwrap();
    assert_eq!(app.session_nav.selected(), Some(0));
    terminal.draw(|f| app.draw(f)).unwrap();
    assert!(!app.areas.sources.iter().any(|(_, link)| *link == Link::SessionPage(4)));
    assert_eq!(app.selected_work.as_ref().unwrap()["session"]["id"], selection);
    app.key(KeyCode::PageUp, KeyModifiers::NONE).unwrap();
    assert_eq!(app.session_page, 2);
    app.change_session_page(0);
    app.key(KeyCode::Home, KeyModifiers::NONE).unwrap();
    app.key(KeyCode::Enter, KeyModifiers::NONE).unwrap();
    assert!(!app.session_nav_focus);
    assert_eq!(app.document.kind, View::SessionWork);
    let mut narrow = Terminal::new(TestBackend::new(60, 18)).unwrap();
    narrow.draw(|f| app.draw(f)).unwrap();
    assert!(app.areas.sessions.is_empty());
    app.key(KeyCode::Tab, KeyModifiers::NONE).unwrap();
    narrow.draw(|f| app.draw(f)).unwrap();
    assert!(!app.areas.sessions.is_empty());
}
#[test]
fn clean_tree_does_not_erase_session_work_and_missing_history_never_uses_today() {
    let (_dir, mut app) = app();
    app.review["changes"] = json!([]);
    app.decision_home();
    assert!(text(&app.document).contains("Persist jobs in PostgreSQL"));
    assert!(
        app.document
            .sources
            .iter()
            .any(|(_, l)| *l == Link::DiscoverDecisions)
    );
    app.sessions.clear();
    app.selected_work = None;
    app.decision_brief = None;
    app.decision_home();
    assert!(text(&app.document).contains("No captured session"));
    assert!(
        !app.document
            .sources
            .iter()
            .any(|(_, l)| *l == Link::DiscoverDecisions)
    );
    assert!(app.job.is_none());
}
#[test]
fn session_catalog_has_no_twenty_session_cap_and_loads_only_the_selection() {
    let (_dir, original) = app();
    let root = original.root.clone();
    for index in 1..=25 {
        let rows = [
            json!({"type":"session_meta","payload":{"id":format!("session-{index}"),"cwd":root,"timestamp":format!("2026-11-{index:02}T10:00:00Z")}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"A request"}]}}),
        ];
        std::fs::write(root.join(format!(".codex/sessions/catalog-{index}.jsonl")), rows.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")).unwrap();
    }
    let mut app = Workspace::new(&root).unwrap();
    assert_eq!(app.navigation_sessions().len(), 26);
    assert_eq!(app.sessions.len(), 1);
    assert_eq!(app.selected_work.as_ref().unwrap()["session"]["id"], "session-25");
    let reference = arr(&app.review["sessions"]).iter().find(|r| r["id"] == "session-1").unwrap();
    assert!(reference["source_entry"].is_object());
    let key = s(&reference["storage_key"]).to_owned();
    app.select_session(&key).unwrap();
    assert_eq!(app.sessions.len(), 1);
    assert_eq!(app.selected_work.as_ref().unwrap()["session"]["id"], "session-1");
    if let Some(history_views::Page::Work { work, .. }) = app.document.pagination.as_ref() {
        assert_eq!(work["indexed_reader"], true);
    } else { panic!("Expected indexed request reader"); }
    std::fs::remove_file(root.join(".codex/sessions/catalog-1.jsonl")).unwrap();
    app.refresh().unwrap();
    assert_eq!(app.navigation_sessions().len(), 26, "Saved sessions remain available without their source files");
    assert_eq!(app.selected_work.as_ref().unwrap()["session"]["id"], "session-1");
}
#[test]
fn startup_selects_latest_capture_and_refresh_preserves_explicit_selection() {
    let (_dir, app) = app();
    let root = app.root.clone();
    let original = std::fs::read_to_string(root.join(".codex/sessions/session.jsonl")).unwrap();
    let newer = original
        .replace("\"id\":\"coding\"", "\"id\":\"newer\"")
        .replace("2026-10-09", "2026-10-10")
        .replace("Persist background jobs", "NEW_SESSION_REQUEST")
        .replace("persist(job)", "NEW_SESSION_CODE");
    std::fs::write(root.join(".codex/sessions/newer.jsonl"), newer).unwrap();
    let mut app = Workspace::new(&root).unwrap();
    assert_eq!(
        app.selected_work.as_ref().unwrap()["session"]["id"],
        "newer"
    );
    assert!(text(&app.document).contains("NEW_SESSION_REQUEST"));
    assert!(!text(&app.document).contains("Persist background jobs"));
    assert_eq!(app.sessions.len(), 1, "Only the selected transcript is loaded");
    let old = arr(&app.review["sessions"]).iter().find(|s| s["id"] == "coding").unwrap();
    let key = s(&old["storage_key"]).to_owned();
    app.follow(Link::Session(key)).unwrap();
    assert_eq!(
        app.selected_work.as_ref().unwrap()["session"]["id"],
        "coding"
    );
    app.refresh().unwrap();
    assert_eq!(
        app.selected_work.as_ref().unwrap()["session"]["id"],
        "coding"
    );
    assert!(text(&app.document).contains("Persist background jobs"));
    assert!(app.job.is_none());
}
#[test]
fn request_cards_have_direct_notes_and_linked_change_groups_without_repeated_warnings() {
    let (_dir, mut app) = app();
    let mut work = app.decision_scope();
    work["turns"][0]["prior_request"] = work["turns"][0]["request"].clone();
    work["turns"][0]["request"]["text"] = json!("yes do it");
    work["turns"][0]["messages"].as_array_mut().unwrap().push(json!({"kind":"rationale","text":"CAPTURED_NOTES: compare the available adapters before choosing an implementation.","provenance":{"source_type":"unknown"}}));
    let edit = work["edits"][0].clone();
    work["edits"] = json!(
        (0..5)
            .map(|_| {
                let mut e = edit.clone();
                e["state"] = json!("recorded");
                e
            })
            .collect::<Vec<_>>()
    );
    work["turns"][0]["edit_indices"] = json!([0, 1, 2, 3, 4]);
    work["turns"].as_array_mut().unwrap().push(json!({"request":{"id":"next","kind":"user","text":"Another request","provenance":{"source_type":"original_turn"}},"messages":[{"kind":"rationale","text":"OTHER_TURN_NOTES"}],"edit_indices":[]}));
    let work = Arc::new(work);
    for width in [40, 80, 140] {
        app.open(session_views::flow(work.clone(), 0));
        let flat = text(&app.document);
        assert!(flat.contains("yes do it"));
        assert!(!flat.contains("Persist background jobs"));
        assert!(flat.contains("Agent response") && flat.contains("Agent notes"));
        assert!(!flat.contains("CAPTURED_NOTES"));
        assert!(!flat.contains("OTHER_TURN_NOTES"));
        assert!(!flat.contains("Implementation flow"));
        assert_eq!(flat.matches("Execution unconfirmed").count(), 1);
        assert!(
            app.document
                .reader_panels()
                .iter()
                .all(|(_, _, request)| *request)
        );
        assert_eq!(app.document.reader_panels().len(), 1);
        assert!(
            !app.document
                .sources
                .iter()
                .any(|(_, l)| matches!(l, Link::SessionEdit(_)))
        );
        let mut terminal = Terminal::new(TestBackend::new(width, 40)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let screen = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(screen.contains('╭') && screen.contains('╰') && screen.contains("yes do it"));
        if let Some(path) = std::env::var_os("WY_TUI_PREVIEW_DIR") {
            std::fs::create_dir_all(&path).unwrap();
            let rows = terminal
                .backend()
                .buffer()
                .content
                .chunks(width as usize)
                .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n");
            std::fs::write(
                Path::new(&path).join(format!("session-request-hierarchy-{width}.txt")),
                rows,
            )
            .unwrap();
        }
        let notes = app
            .areas
            .sources
            .iter()
            .find(|(_, l)| *l == Link::Disclosure("session-turn-0-0-notes".into()))
            .unwrap()
            .0;
        app.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: notes.x,
            row: notes.y,
            modifiers: KeyModifiers::NONE,
        })
        .unwrap();
        assert!(text(&app.document).contains("CAPTURED_NOTES"));
        assert!(!text(&app.document).contains("OTHER_TURN_NOTES"));
        let changes = "session-turn-0-0-changes".to_owned();
        app.document.source_selection = app
            .document
            .sources
            .iter()
            .position(|(_, l)| *l == Link::Disclosure(changes.clone()));
        app.key(KeyCode::Enter, KeyModifiers::NONE).unwrap();
        assert_eq!(
            app.document
                .sources
                .iter()
                .filter(|(_, l)| matches!(l, Link::SessionEdit(_)))
                .count(),
            5
        );
        assert!(app.document.sources.windows(2).all(|p| p[0].0 <= p[1].0));
        let expected = Link::SessionEdit(crate::history::edit_ref(&edit));
        assert!(app.document.sources.iter().any(|(_, l)| *l == expected));
        // Closing a group while its child is selected returns selection to the
        // header and removes every hidden edit from keyboard/mouse navigation.
        app.document.source_selection = app
            .document
            .sources
            .iter()
            .rposition(|(_, l)| matches!(l, Link::SessionEdit(_)));
        app.follow(Link::Disclosure(changes.clone())).unwrap();
        assert_eq!(
            app.document.sources[app.document.source_selection.unwrap()].1,
            Link::Disclosure(changes)
        );
        assert!(
            !app.document
                .sources
                .iter()
                .any(|(_, l)| matches!(l, Link::SessionEdit(_)))
        );
        app.follow(Link::Disclosure("session-turn-0-0-earlier".into()))
            .unwrap();
        assert!(text(&app.document).contains("Persist background jobs"));
        assert!(text(&app.document).contains("yes do it"));
        assert!(text(&app.document).contains("context only"));
        assert!(app.job.is_none());
    }
}

#[test]
fn request_detail_keeps_all_edits_together_without_splitting_the_request() {
    let (_dir, mut app) = app();
    let mut work = app.decision_scope();
    let edit = work["edits"][0].clone();
    work["edits"] = json!(
        (0..45)
            .map(|i| {
                let mut e = edit.clone();
                e["file"] = json!(format!("step-{i}.rs"));
                e
            })
            .collect::<Vec<_>>()
    );
    work["turns"][0]["edit_indices"] = json!((0..45).collect::<Vec<_>>());
    let expand = |app: &mut Workspace| {
        let id = app
            .document
            .sources
            .iter()
            .find_map(|(_, l)| {
                if let Link::Disclosure(id) = l {
                    id.ends_with("-changes").then_some(id.clone())
                } else {
                    None
                }
            })
            .unwrap();
        app.follow(Link::Disclosure(id)).unwrap();
    };
    app.open(session_views::flow(Arc::new(work), 0));
    expand(&mut app);
    assert!(text(&app.document).contains("step-0.rs"));
    assert!(text(&app.document).contains("step-20.rs"));
    assert!(text(&app.document).contains("step-44.rs"));
    assert!(!text(&app.document).contains("Your request · continued"));
    assert_eq!(app.request_position(), Some((0, 1)));
    assert_eq!(app.document.title, "Detail");
}
#[test]
fn session_and_request_panes_resize_and_persist_without_changing_detail() {
    let (dir, mut app) = app();
    app.session_flow();
    let selected = app.request_position();
    let mut terminal = Terminal::new(TestBackend::new(160, 32)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    for (divider, delta) in [(Divider::Sessions, 8i16), (Divider::Requests, -8)] {
        let rect = match divider { Divider::Sessions => app.areas.session_divider, _ => app.areas.request_divider };
        assert_eq!(rect.width, 1);
        let target = rect.x.saturating_add_signed(delta);
        for (kind, column) in [(MouseEventKind::Down(MouseButton::Left), rect.x), (MouseEventKind::Drag(MouseButton::Left), target)] {
            app.mouse(MouseEvent { kind, column, row: rect.y + 2, modifiers: KeyModifiers::NONE }).unwrap();
        }
        terminal.draw(|f| app.draw(f)).unwrap();
        let moved = match divider { Divider::Sessions => app.areas.session_divider, _ => app.areas.request_divider };
        assert_eq!(moved.x, target);
        app.mouse(MouseEvent { kind: MouseEventKind::Up(MouseButton::Left), column: target, row: rect.y + 2, modifiers: KeyModifiers::NONE }).unwrap();
        assert_eq!(app.request_position(), selected);
        assert!(app.areas.reader.width >= 40);
    }
    let saved = PaneSizes::load(dir.path()).unwrap();
    assert_eq!(saved.sessions, app.pane_sizes.sessions);
    assert_eq!(saved.requests, app.pane_sizes.requests);
    app.key(KeyCode::BackTab, KeyModifiers::NONE).unwrap();
    let width = app.pane_sizes.requests.unwrap();
    app.key(KeyCode::Char(']'), KeyModifiers::NONE).unwrap();
    assert_eq!(app.pane_sizes.requests, Some(width + 3));
    assert!(app.request_nav_focus);
    let mut smaller = Terminal::new(TestBackend::new(110, 24)).unwrap();
    smaller.draw(|f| app.draw(f)).unwrap();
    assert!(app.areas.reader.width >= 40);
    app.resize_pane(Divider::Sessions, u16::MAX);
    smaller.draw(|f| app.draw(f)).unwrap();
    assert!(app.areas.reader.width >= 40);
    app.resize_pane(Divider::Requests, 0);
    smaller.draw(|f| app.draw(f)).unwrap();
    assert!(app.areas.reader.width >= 40);
    assert_eq!(app.request_position(), selected);
    let screen = terminal.backend().buffer().content.iter().map(|cell| cell.symbol()).collect::<String>();
    assert!(screen.contains("Explain with: codex"));
}
#[test]
fn session_responses_and_notes_keep_text_beyond_previous_capture_and_display_limits() {
    let (_dir, mut app) = app();
    let path = app.root.join(".codex/sessions/long.jsonl");
    let response = format!("{} RESPONSE_END", "response ".repeat(5000));
    let notes = format!("{} NOTES_END", "notes ".repeat(7000));
    let rows = [
        json!({"type":"session_meta","payload":{"id":"long","cwd":app.root}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Show full content"}]}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":response}]}}),
        json!({"type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":notes}]}}),
    ];
    std::fs::write(&path, rows.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")).unwrap();
    let session = crate::history::collect(&path).unwrap();
    assert!(arr(&session["events"]).iter().any(|e| e["text"] == response && e["truncated"] == false));
    assert!(arr(&session["events"]).iter().any(|e| e["text"] == notes && e["truncated"] == false));
    let work = crate::session_work::build(&app.review, &session);
    app.open(session_views::flow(Arc::new(work), 0));
    for kind in ["response", "notes"] {
        app.follow(Link::Disclosure(format!("session-turn-0-0-{kind}"))).unwrap();
    }
    let detail = text(&app.document);
    assert!(detail.contains(&response));
    assert!(detail.contains(&notes));
    assert!(!detail.contains("Partial excerpt"));
    assert!(!detail.contains("Additional captured messages omitted"));
}
#[test]
fn request_labels_use_captured_ids_not_message_or_project_text() {
    let turn = json!({"request":{"id":"event-42","kind":"user","text":"<environment_context>Project: /private/project</environment_context>"}});
    assert_eq!(session_flow::request_label(&turn, 0), "event-42");
    assert_eq!(session_flow::request_label(&json!({"request":{"text":"/private/project"}}), 20), "Request 21 · ID unavailable");
    assert_eq!(session_flow::request_label(&json!({"request":null}), 0), "Request 1 · ID unavailable");
}
#[test]
fn requests_pane_paginates_independently_and_restores_selection() {
    let (_dir, mut app) = app();
    let mut work = app.decision_scope();
    let turn = work["turns"][0].clone();
    work["turns"] = json!((0..45).map(|index| {
        let mut turn = turn.clone();
        turn["request"]["text"] = json!(format!("REQUEST_{index:02}"));
        turn
    }).collect::<Vec<_>>());
    app.selected_work = Some(work.clone());
    app.open(session_views::flow(Arc::new(work), 0));
    let mut terminal = Terminal::new(TestBackend::new(140, 32)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    assert!(app.areas.sessions.right() <= app.areas.reader.x);
    assert!(app.areas.reader.right() <= app.areas.requests.x);
    assert!(text(&app.document).contains("REQUEST_00"));
    assert!(!text(&app.document).contains("REQUEST_01"));
    // Next-page links live in the right pane, not the detail document.
    assert!(!app.document.sources.iter().any(|(_, link)| matches!(link, Link::Page(_))));
    let next = app.areas.sources.iter().find(|(_, link)| *link == Link::Page(20)).unwrap().0;
    app.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: next.x, row: next.y, modifiers: KeyModifiers::NONE }).unwrap();
    assert_eq!(app.request_position(), Some((20, 45)));
    assert!(text(&app.document).contains("REQUEST_20"));
    app.key(KeyCode::BackTab, KeyModifiers::NONE).unwrap();
    assert!(app.request_nav_focus);
    app.key(KeyCode::Down, KeyModifiers::NONE).unwrap();
    app.key(KeyCode::Enter, KeyModifiers::NONE).unwrap();
    assert_eq!(app.request_position(), Some((21, 45)));
    app.decision_home();
    app.session_flow();
    assert_eq!(app.request_position(), Some((21, 45)));
    app.key(KeyCode::Right, KeyModifiers::ALT).unwrap();
    assert_eq!(app.request_position(), Some((40, 45)));
    app.key(KeyCode::BackTab, KeyModifiers::NONE).unwrap();
    app.key(KeyCode::End, KeyModifiers::NONE).unwrap();
    app.key(KeyCode::Enter, KeyModifiers::NONE).unwrap();
    assert_eq!(app.request_position(), Some((44, 45)));
    terminal.draw(|f| app.draw(f)).unwrap();
    assert!(!app.areas.sources.iter().any(|(_, link)| *link == Link::Page(60)));
    let mut narrow = Terminal::new(TestBackend::new(60, 18)).unwrap();
    narrow.draw(|f| app.draw(f)).unwrap();
    assert!(app.areas.requests.is_empty());
    app.key(KeyCode::BackTab, KeyModifiers::NONE).unwrap();
    narrow.draw(|f| app.draw(f)).unwrap();
    assert!(!app.areas.requests.is_empty());
}
#[test]
fn working_tree_changes_are_explicitly_unassigned_to_selected_session() {
    let (_dir, mut app) = app();
    let scope = crate::decisions::scope_key(&app.decision_scope());
    app.open(session_views::working_changes(&app.review));
    assert!(app.document.title.contains("attribution unknown"));
    assert!(text(&app.document).contains("not assigned to the selected session"));
    assert_eq!(crate::decisions::scope_key(&app.decision_scope()), scope);
}
