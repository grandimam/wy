use super::*;
use ratatui::backend::TestBackend;
use serde_json::json;

fn git(root: &Path, args: &[&str]) {
    crate::repository::git(root, args, true).unwrap();
}
fn rows(root: &Path) -> Vec<Value> {
    let patch = |call: &str, file: &str, body: &str| {
        [
            json!({"type":"response_item","timestamp":"2026-10-11T08:10:00Z","payload":{"type":"custom_tool_call","name":"apply_patch","call_id":call,"input":format!("*** Begin Patch\n*** Add File: {file}\n{}\n*** End Patch", body.lines().map(|l| format!("+{l}")).collect::<Vec<_>>().join("\n"))}}),
            json!({"type":"response_item","timestamp":"2026-10-11T08:10:01Z","payload":{"type":"function_call_output","call_id":call,"output":"Success. Updated the following files:"}}),
        ]
    };
    let mut rows = vec![
        json!({"type":"session_meta","payload":{"id":"hooks","cwd":root,"timestamp":"2026-10-11T08:00:00Z"}}),
        json!({"type":"response_item","timestamp":"2026-10-11T08:01:00Z","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Add webhook dedup"}]}}),
        json!({"type":"response_item","timestamp":"2026-10-11T08:02:00Z","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"I'll persist the event id so that duplicate deliveries are ignored."}]}}),
    ];
    rows.extend(patch(
        "c1",
        "src/hook.rs",
        "fn handle() {\n    if seen(id) { return; }\n    record(id);\n}",
    ));
    rows.push(json!({"type":"response_item","timestamp":"2026-10-11T08:10:30Z","payload":{"type":"function_call","name":"shell","call_id":"t1","arguments":json!({"command":["cargo","test"]}).to_string()}}));
    rows.push(json!({"type":"response_item","timestamp":"2026-10-11T08:10:31Z","payload":{"type":"function_call_output","call_id":"t1","output":"test result: ok. 2 passed; 0 failed"}}));
    rows.push(json!({"type":"response_item","timestamp":"2026-10-11T08:11:00Z","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Now I'll add the mailer."}]}}));
    rows.extend(patch("c2", "src/mail.rs", "fn send() {}"));
    rows
}
/// Repository with a committed base, one captured session, and a dirty tree the
/// session accounts for, plus one manual file no session touched.
fn app() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "t@example.com"]);
    git(&root, &["config", "user.name", "t"]);
    std::fs::write(root.join("README.md"), "base\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "base"]);
    let path = root.join(".codex/sessions/hooks.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        rows(&root)
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/hook.rs"),
        "fn handle() {\n    if seen(id) { return; }\n    record(id);\n}\n",
    )
    .unwrap();
    std::fs::write(root.join("src/mail.rs"), "fn send() {}\n").unwrap();
    std::fs::write(root.join("src/manual.rs"), "fn by_hand() {}\n").unwrap();
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
    app.rebuild_lenses();
    app.plan_home();
    (dir, app)
}
fn text(doc: &Document) -> String {
    doc.lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}
fn brief(app: &Workspace) -> Value {
    let sessions = app.evidence_sessions();
    let packet = crate::judgment::packet(&app.review, &app.hunks, &app.steps, &sessions);
    let hook = arr(&packet["hunks"])
        .iter()
        .find(|h| h["file"] == "src/hook.rs")
        .unwrap()
        .clone();
    let manual = arr(&packet["hunks"])
        .iter()
        .find(|h| h["file"] == "src/manual.rs")
        .unwrap()
        .clone();
    let statement = arr(&packet["statements"])
        .iter()
        .find(|st| s(&st["text"]).contains("duplicate"))
        .unwrap()
        .clone();
    let questions = vec![
        json!({"question":"What happens if the same event arrives twice?","domain_why":"Webhooks deliver at least once.","consequence":"high","consequence_note":"Double processing.",
            "chosen":"Dedup by id.","said":{"status":"recorded","quote":"duplicate deliveries are ignored","quote_id":statement["id"],"inferred_from":[]},
            "check":[{"hunk":hook["id"],"line":2,"note":"the early return"}],"expert_view":"","alternatives":[],"hunks":[hook["id"]]}),
        json!({"question":"Who wrote manual.rs?","domain_why":"Unattributed code.","consequence":"low","consequence_note":"",
            "chosen":"A helper.","said":{"status":"unknown","quote":"","quote_id":"","inferred_from":[]},
            "check":[],"expert_view":"Ask the author.","alternatives":[],"hunks":[manual["id"]]}),
    ];
    for q in &questions {
        crate::judgment::validate_question(q, &packet).unwrap();
    }
    let mut questions = questions;
    for q in &mut questions {
        q["key"] = json!(crate::judgment::question_key(q));
    }
    json!({"id":"judgment-test","context":"judgment_brief","review_id":app.review["id"],"scope_key":app.judgment_key(),
        "hunk_key":crate::hunks::key(&app.hunks),"head":app.review["head"],"created_at":crate::now(),"mode":"assessment","agent":"codex",
        "session_key":app.session_key(),"packet":packet,"questions":questions,"rejected":["bogus — cites a hunk outside the diff"],"unknowns":[]})
}

#[test]
fn dirty_tree_opens_the_plan_with_every_row_openable_and_nothing_running() {
    let (_dir, mut app) = app();
    assert_eq!(app.document.kind, View::Plan);
    assert_eq!(app.section(), View::Plan);
    assert!(!app.sidebar);
    assert!(app.job.is_none());
    let body = text(&app.document);
    assert!(body.contains("What changed"));
    assert!(body.contains("What to worry about"));
    assert!(body.contains("Was it tested"));
    assert!(body.contains("Progress"));
    // Step titles are what the agent did, not what it said.
    assert!(body.contains("Persist the event id"), "{body}");
    assert!(body.contains("Add the mailer"));
    assert!(body.contains(
        "what a domain expert would ask about this diff · Enter asks codex · nothing has run"
    ));
    assert!(body.contains("1 file changed with no record of why (src/manual.rs)"));
    assert!(body.contains("tests ran 1 time · no failures · last run passed"));
    assert!(body.contains("Not tested: add the mailer"));
    assert!(body.contains("0 of 2 changes read"));
    // Attribution is textual: the session's files match, the manual file does not.
    let by =
        |file: &str| app.hunks.iter().find(|h| h["file"] == file).unwrap()["attribution"].clone();
    assert_eq!(by("src/hook.rs"), "matched");
    assert_eq!(by("src/mail.rs"), "matched");
    assert_eq!(by("src/manual.rs"), "none");
    // Rows: the evidence line first, then the change groups. Enter opens what is under the cursor.
    assert!(body.contains("evidence: codex “Add webhook dedup”"));
    app.key(KeyCode::Enter, KeyModifiers::NONE).unwrap();
    assert_eq!(app.document.kind, View::PlanGap, "evidence detail");
    assert!(text(&app.document).contains("Chosen from the diff"));
    assert!(text(&app.document).contains("explains 2 hunks"));
    app.key(KeyCode::Esc, KeyModifiers::NONE).unwrap();
    app.document.source_selection = Some(1);
    app.key(KeyCode::Enter, KeyModifiers::NONE).unwrap();
    assert_eq!(app.document.kind, View::PlanChange);
    let change = text(&app.document);
    assert!(change.contains("Why, in the agent's words"));
    assert!(change.contains("duplicate deliveries are ignored"));
    assert!(change.contains("record(id);"));
    assert!(change.contains("✓ cargo test"));
    app.key(KeyCode::Esc, KeyModifiers::NONE).unwrap();
    assert_eq!(app.document.kind, View::Plan);
    assert!(text(&app.document).contains("1 of 2 changes read"));
    assert!(text(&app.document).contains("✓ Persist the event id"));
    // The gap row and the tests row open their evidence.
    app.follow(Link::Gap("unexplained".into())).unwrap();
    assert_eq!(app.document.kind, View::PlanGap);
    assert!(text(&app.document).contains("src/manual.rs · 1 hunk"));
    app.follow(Link::Tests).unwrap();
    assert_eq!(app.document.kind, View::PlanTests);
    assert!(text(&app.document).contains("✓ step 1  test   cargo test"));
    assert!(text(&app.document).contains("✗ Add the mailer"));
    // Two tabs only; the transcript is a drill-down.
    for width in [40, 80, 140] {
        let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let views: Vec<_> = app.areas.tabs.iter().map(|(_, v, _)| *v).collect();
        assert_eq!(views, vec![View::Plan, View::Steps]);
    }
}

#[test]
fn steps_show_why_check_gap_and_plan_rows_and_link_to_the_transcript() {
    let (_dir, mut app) = app();
    app.judgment_brief = Some(Arc::new(brief(&app)));
    app.key(KeyCode::Char('t'), KeyModifiers::NONE).unwrap();
    assert_eq!(app.document.kind, View::Steps);
    assert_eq!(app.section(), View::Steps);
    let body = text(&app.document);
    assert!(body.contains("1  08:02  Persist the event id  ── hook.rs"));
    assert!(
        body.contains(
            "Why    “I'll persist the event id so that duplicate deliveries are ignored.”"
        )
    );
    assert!(body.contains("Check  ✓ cargo test"));
    assert!(body.contains("Why    no reason given"));
    assert!(body.contains("Check  ✗ no build or test run"));
    assert!(body.contains("Plan   HIGH  What happens if the same event arrives twice?"));
    assert!(body.contains("Not explained by this session"));
    assert!(body.contains("src/manual.rs · 1 hunk · no captured edit matches"));
    app.document.source_selection = Some(1);
    app.key(KeyCode::Enter, KeyModifiers::NONE).unwrap();
    assert_eq!(app.document.kind, View::Step);
    let step = text(&app.document);
    assert!(step.contains("Why it said it did it"));
    assert!(step.contains("exact quote"));
    assert!(step.contains("Worries this step raises"));
    assert!(step.contains("record(id);"));
    let transcript = app
        .document
        .sources
        .iter()
        .find(|(_, l)| matches!(l, Link::Transcript(_)))
        .unwrap()
        .1
        .clone();
    app.follow(transcript).unwrap();
    assert_eq!(app.document.kind, View::SessionWork);
    assert_eq!(
        app.section(),
        View::Steps,
        "the transcript lives under Steps"
    );
    app.key(KeyCode::Char('p'), KeyModifiers::NONE).unwrap();
    assert_eq!(app.document.kind, View::Plan);
}

#[test]
fn worries_show_code_with_markers_and_cross_link_to_steps() {
    let (_dir, mut app) = app();
    app.judgment_brief = Some(Arc::new(brief(&app)));
    app.plan_home();
    let body = text(&app.document);
    assert!(
        body.find("same event arrives twice").unwrap() < body.find("Who wrote manual.rs").unwrap()
    );
    assert!(body.contains("HIGH") && body.contains("recorded"));
    assert!(body.contains("1 formed question was rejected"));
    assert!(body.contains("0 of 2 worries decided"));
    app.follow(Link::Question(0)).unwrap();
    assert_eq!(app.document.kind, View::Worry);
    assert_eq!(app.section(), View::Plan);
    let detail = text(&app.document);
    assert!(detail.contains("Why this matters here"));
    assert!(detail.contains("“duplicate deliveries are ignored”"));
    assert!(detail.contains("(exact quote)"));
    assert!(detail.contains("step 1"));
    assert!(detail.contains("if seen(id) { return; }    ◄ the early return"));
    assert!(detail.contains("→ src/hook.rs:2   the early return"));
    assert!(detail.contains("Your verdict"));
    let step = app
        .document
        .sources
        .iter()
        .find(|(_, l)| matches!(l, Link::Step(0)))
        .unwrap()
        .1
        .clone();
    app.follow(step).unwrap();
    assert_eq!(app.document.kind, View::Step);
    let back = app
        .document
        .sources
        .iter()
        .find(|(_, l)| matches!(l, Link::Question(0)))
        .unwrap()
        .1
        .clone();
    app.follow(back).unwrap();
    assert_eq!(app.document.kind, View::Worry);
    app.follow(Link::Question(1)).unwrap();
    let unknown = text(&app.document);
    assert!(unknown.contains("Nothing. No recorded statement"));
    assert!(unknown.contains("⚠ no captured edit for this hunk"));
    assert!(unknown.contains("What an expert would want here · wy's assessment"));
    let hunk = app
        .document
        .sources
        .iter()
        .find(|(_, l)| matches!(l, Link::Hunk(_)))
        .unwrap()
        .1
        .clone();
    app.follow(hunk).unwrap();
    assert_eq!(app.document.kind, View::Diff);
    assert_eq!(app.document.target.as_ref().unwrap().file, "src/manual.rs");
}

#[test]
fn verdicts_persist_render_stale_and_export_in_the_reviewers_voice() {
    let (_dir, mut app) = app();
    app.judgment_brief = Some(Arc::new(brief(&app)));
    app.plan_home();
    app.follow(Link::Question(0)).unwrap();
    app.document.scroll = 3;
    app.key(KeyCode::Char('n'), KeyModifiers::NONE).unwrap();
    assert_eq!(app.document.kind, View::Worry);
    assert_eq!(app.document.scroll, 3, "verdict re-renders in place");
    assert!(text(&app.document).contains("● push back"));
    app.key(KeyCode::Char('i'), KeyModifiers::NONE).unwrap();
    assert_eq!(app.editing, Some(Input::Note));
    app.input = "need a transaction around record(id)".into();
    app.key(KeyCode::Enter, KeyModifiers::NONE).unwrap();
    assert!(app.editing.is_none());
    assert!(text(&app.document).contains("note: need a transaction"));
    app.follow(Link::Question(1)).unwrap();
    app.key(KeyCode::Char('y'), KeyModifiers::NONE).unwrap();
    assert!(text(&app.document).contains("● accept"));
    assert_eq!(crate::verdicts::load(&app.root).unwrap().len(), 2);
    app.plan_home();
    let overview = text(&app.document);
    assert!(overview.contains("✗ HIGH"));
    assert!(overview.contains("push back"));
    assert!(
        overview.contains("2 of 2 worries decided · 1 pushed back · /export writes the review")
    );
    app.input = "/export".into();
    app.editing = Some(Input::Command);
    app.command().unwrap();
    assert_eq!(app.document.kind, View::Export);
    let export = s(&app.document.artifact.as_ref().unwrap()["export_text"]).to_owned();
    assert!(export.starts_with("# Review: Add webhook dedup"));
    assert!(export.find("## Push back (1)").unwrap() < export.find("## Accepted (1)").unwrap());
    assert!(export.contains("Reviewer: need a transaction around record(id)"));
    // Editing the cited code marks the verdict stale on both screens, never clears it.
    std::fs::write(
        app.root.join("src/hook.rs"),
        "fn handle() {\n    if seen(id) { return; }\n    record(id);\n    audit();\n}\n",
    )
    .unwrap();
    app.review = service::review(
        &app.root,
        &service::ReviewOptions {
            source: "codex".into(),
        },
    )
    .unwrap();
    app.rebuild_lenses();
    app.plan_home();
    let stale = text(&app.document);
    assert!(stale.contains("the code changed since these were formed · Enter re-forms them"));
    assert!(stale.contains("push back · stale"));
    app.follow(Link::Question(0)).unwrap();
    assert!(text(&app.document).contains("changed after your verdict"));
    assert!(
        text(&app.document).contains("audit();"),
        "the hunk shown is the current one"
    );
}

#[test]
fn asking_is_a_row_you_walk_into_and_results_land_only_for_the_same_tree() {
    let (_dir, mut app) = app();
    // The ask row is the first row of "What to worry about": after the two change rows.
    let ask = app
        .document
        .sources
        .iter()
        .position(|(_, l)| *l == Link::GenerateQuestions)
        .unwrap();
    app.document.source_selection = Some(ask);
    app.key(KeyCode::Enter, KeyModifiers::NONE).unwrap();
    assert!(app.job.is_some());
    assert!(text(&app.document).contains("forming the questions a domain expert would ask"));
    let key = app.job.take().unwrap().key;
    assert!(key.starts_with("judgment:"));
    let job = |key: &str| {
        let (sender, receiver) = mpsc::channel();
        (
            sender,
            Job {
                receiver,
                cancel: Arc::new(AtomicBool::new(false)),
                handle: thread::spawn(|| {}),
                started: Instant::now(),
                scope: "judgment questions".into(),
                key: key.into(),
                file: None,
                progress: String::new(),
            },
        )
    };
    let (sender, j) = job(&key);
    app.job = Some(j);
    let mut other = brief(&app);
    other["scope_key"] = json!("elsewhere");
    sender.send(Update::Done(Ok(other))).unwrap();
    app.poll();
    assert!(app.judgment_brief.is_none());
    assert!(app.status.contains("another capture"));
    let (sender, j) = job(&key);
    app.job = Some(j);
    sender
        .send(Update::Done(Err(anyhow::anyhow!("CLI missing"))))
        .unwrap();
    app.poll();
    assert!(text(&app.document).contains("could not form questions: CLI missing · Enter retries"));
    assert!(app.error);
    let (sender, j) = job(&key);
    app.job = Some(j);
    sender.send(Update::Done(Ok(brief(&app)))).unwrap();
    app.poll();
    assert!(app.judgment_brief.is_some());
    let body = text(&app.document);
    assert!(body.contains("same event arrives twice"));
    assert!(body.contains("0 of 2 worries decided"));
}

#[test]
fn clean_tree_says_nothing_to_accept_and_steps_survive_the_commit() {
    let (_dir, mut app) = app();
    git(&app.root, &["add", "-A"]);
    git(&app.root, &["commit", "-q", "-m", "accept"]);
    app.review = service::review(
        &app.root,
        &service::ReviewOptions {
            source: "codex".into(),
        },
    )
    .unwrap();
    app.rebuild_lenses();
    assert!(app.hunks.is_empty());
    app.plan_home();
    assert!(text(&app.document).contains("Nothing uncommitted. There is nothing to accept yet."));
    app.steps_home();
    assert_eq!(app.steps.len(), 2);
    app.open_step(0).unwrap();
    assert!(text(&app.document).contains("no longer in the working tree"));
}

/// A second session that touched the same file earlier: evidence is chosen by what
/// it explains, newest first, so the attempt that produced the tree wins outright.
#[test]
fn evidence_is_chosen_from_the_diff_and_can_be_overridden_by_hand() {
    let (_dir, mut app) = app();
    let root = app.root.clone();
    let original = std::fs::read_to_string(root.join(".codex/sessions/hooks.jsonl")).unwrap();
    // An older attempt wrote a different hook.rs and a helper that is still in the tree.
    let older = original
        .replace("\"id\":\"hooks\"", "\"id\":\"attempt\"")
        .replace("2026-10-11T08", "2026-10-10T08")
        .replace("Add webhook dedup", "First attempt")
        .replace("record(id);", "remember(id);")
        .replace("src/mail.rs", "src/manual.rs")
        .replace("fn send() {}", "fn by_hand() {}");
    std::fs::write(root.join(".codex/sessions/attempt.jsonl"), older).unwrap();
    app.review = service::review(
        &root,
        &service::ReviewOptions {
            source: "codex".into(),
        },
    )
    .unwrap();
    app.sessions = crate::history::saved(&app.review).unwrap();
    app.rebuild_lenses();
    // Both explain something: hooks explains hook.rs + mail.rs, attempt explains manual.rs.
    let ids: Vec<String> = app
        .evidence
        .iter()
        .map(|(sn, _, _)| s(&sn["id"]).to_owned())
        .collect();
    assert_eq!(
        ids,
        vec!["attempt", "hooks"],
        "oldest first for chronological steps"
    );
    assert!(app.hunks.iter().all(|h| h["attribution"] == "matched"));
    app.plan_home();
    let body = text(&app.document);
    assert!(body.contains("evidence: 2 sessions explain 3 of 3 files"));
    assert!(body.contains("⟨codex · First attempt⟩"));
    assert!(!body.contains("no record of why"));
    app.follow(Link::Evidence).unwrap();
    let detail = text(&app.document);
    // The older attempt also partly matches hook.rs (shared lines): textual match, honestly reported.
    assert!(detail.contains("“First attempt” · ") && detail.contains("“Add webhook dedup” · "));
    assert_eq!(detail.matches("explains 2 hunks").count(), 2);
    // Hand-pick the older one: it becomes the only evidence, and the diff can take over again.
    let key = crate::session_work::snapshot_key(
        &app.sessions
            .iter()
            .find(|sn| sn["id"] == "attempt")
            .unwrap()
            .clone(),
    );
    app.follow(Link::Session(key)).unwrap();
    assert_eq!(app.evidence.len(), 1);
    assert_eq!(app.evidence[0].0["id"], "attempt");
    app.plan_home();
    assert!(text(&app.document).contains("chosen by hand"));
    assert!(text(&app.document).contains("1 file changed with no record of why (src/mail.rs)"));
    app.follow(Link::Evidence).unwrap();
    assert!(text(&app.document).contains("Chosen by hand"));
    app.follow(Link::AutoEvidence).unwrap();
    assert_eq!(app.evidence.len(), 2);
    assert!(app.pinned_session.is_none());
    // Steps merge chronologically and carry their session.
    app.steps_home();
    let steps = text(&app.document);
    assert!(steps.contains("2 sessions"));
    assert!(
        steps.find("⟨codex · First attempt⟩").unwrap()
            < steps.find("⟨codex · Add webhook dedup⟩").unwrap()
    );
}
