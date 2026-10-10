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
fn only_decisions_and_sessions_are_primary_and_older_sessions_require_the_picker() {
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
    let old = app.sessions.iter().find(|s| s["id"] == "coding").unwrap();
    let key = crate::session_work::snapshot_key(old);
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
        assert_eq!(app.document.reader_panels().len(), 2);
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
fn implementation_flow_pages_without_discarding_repeated_edits() {
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
    assert!(!text(&app.document).contains("step-20.rs"));
    app.follow(Link::Page(1)).unwrap();
    expand(&mut app);
    assert!(text(&app.document).contains("step-20.rs"));
    assert!(text(&app.document).contains("Your request · continued"));
    app.follow(Link::Page(2)).unwrap();
    expand(&mut app);
    assert!(text(&app.document).contains("step-44.rs"));
    app.follow(Link::Page(0)).unwrap();
    expand(&mut app);
    assert!(text(&app.document).contains("step-0.rs"));
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
