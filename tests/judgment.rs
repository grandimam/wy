//! The hunk spine, step segmentation, question validation and verdicts.
use serde_json::{Value, json};
use std::{fs, path::Path};
use wy::{arr, history, hunks, judgment, n, repository, s, service, session_work, steps, verdicts};

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    repository::git(dir.path(), &["init", "-q"], true).unwrap();
    repository::git(dir.path(), &["config", "user.email", "t@example.com"], true).unwrap();
    repository::git(dir.path(), &["config", "user.name", "t"], true).unwrap();
    dir
}
fn commit(root: &Path) {
    repository::git(root, &["add", "-A"], true).unwrap();
    repository::git(root, &["commit", "-q", "-m", "base", "--allow-empty"], true).unwrap();
}
fn review(root: &Path) -> Value {
    service::review(
        root,
        &service::ReviewOptions {
            source: "codex".into(),
        },
    )
    .unwrap()
}
fn user(t: &str, text: &str) -> Value {
    json!({"type":"response_item","timestamp":t,"payload":{"type":"message","role":"user","content":[{"type":"input_text","text":text}]}})
}
fn assistant(t: &str, text: &str) -> Value {
    json!({"type":"response_item","timestamp":t,"payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}})
}
fn add_file(t: &str, call: &str, file: &str, body: &str) -> [Value; 2] {
    [
        json!({"type":"response_item","timestamp":t,"payload":{"type":"custom_tool_call","name":"apply_patch","call_id":call,"input":format!("*** Begin Patch\n*** Add File: {file}\n{}\n*** End Patch", body.lines().map(|l| format!("+{l}")).collect::<Vec<_>>().join("\n"))}}),
        json!({"type":"response_item","timestamp":t,"payload":{"type":"function_call_output","call_id":call,"output":"Success. Updated the following files:"}}),
    ]
}
fn shell(t: &str, call: &str, command: &str, output: &str) -> [Value; 2] {
    [
        json!({"type":"response_item","timestamp":t,"payload":{"type":"function_call","name":"shell","call_id":call,"arguments":json!({"command":[command]}).to_string()}}),
        json!({"type":"response_item","timestamp":t,"payload":{"type":"function_call_output","call_id":call,"output":output}}),
    ]
}
fn write_session(root: &Path, rows: Vec<Value>) -> Value {
    let path = root.join(".codex/sessions/s.jsonl");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut all = vec![
        json!({"type":"session_meta","payload":{"id":"s","cwd":root,"timestamp":"2026-10-11T08:00:00Z"}}),
    ];
    all.extend(rows);
    fs::write(
        &path,
        all.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    history::collect(&path).unwrap()
}

/// A webhook-handler session: dependency, signature check, dedup, then an untested
/// subscription update, with one file written via shell that has no captured content.
fn webhook_repo() -> (tempfile::TempDir, Value, Value, Value) {
    let dir = repo();
    let root = dir.path().canonicalize().unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname = \"app\"\n").unwrap();
    fs::write(root.join("Cargo.lock"), "# lock v1\n").unwrap();
    commit(&root);
    let dedup = "fn handle(event: Event) {\n    if WebhookEvent::exists(&db, &event.id) {\n        return Ok(());\n    }\n    WebhookEvent::insert(&db, &event.id);\n    Subscription::activate(&db, event.sub_id);\n}";
    let mut rows = vec![user(
        "2026-10-11T08:01:00Z",
        "Add Stripe webhook handling so subscriptions update",
    )];
    rows.push(assistant(
        "2026-10-11T08:02:00Z",
        "I'll add the stripe dependency first.",
    ));
    rows.extend(add_file(
        "2026-10-11T08:02:30Z",
        "c1",
        "Cargo.toml",
        "[package]\nname = \"app\"\n[dependencies]\nstripe = \"0.29\"",
    ));
    rows.push(assistant("2026-10-11T08:05:00Z", "Now I'll verify the Stripe-Signature header before parsing anything, because unsigned requests must be rejected."));
    rows.extend(add_file(
        "2026-10-11T08:05:30Z",
        "c2",
        "src/signature.rs",
        "pub fn verify(sig: &str) -> bool { sig.len() > 8 }",
    ));
    rows.extend(shell(
        "2026-10-11T08:06:00Z",
        "c3",
        "cargo test",
        "running 3 tests\ntest result: ok. 3 passed; 0 failed",
    ));
    rows.push(assistant("2026-10-11T08:12:00Z", "Next I'll persist the event id so that duplicate deliveries are ignored. Stripe retries for 72 hours, so the unique index is the idempotency boundary."));
    rows.extend(add_file(
        "2026-10-11T08:12:30Z",
        "c4",
        "src/webhooks/stripe.rs",
        dedup,
    ));
    rows.push(json!({"type":"response_item","timestamp":"2026-10-11T08:13:00Z","payload":{"type":"function_call","name":"shell","call_id":"c5","arguments":json!({"command":["bash","-lc","echo 'CREATE TABLE webhook_events' > migrations/0007.sql"]}).to_string()}}));
    rows.push(json!({"type":"response_item","timestamp":"2026-10-11T08:13:01Z","payload":{"type":"function_call_output","call_id":"c5","output":""}}));
    rows.extend(shell(
        "2026-10-11T08:14:00Z",
        "c6",
        "cargo build",
        "Compiling app\nFinished",
    ));
    rows.push(assistant(
        "2026-10-11T08:19:00Z",
        "Let me update the subscription repository.",
    ));
    rows.extend(add_file("2026-10-11T08:19:30Z", "c7", "src/subscriptions/repo.rs", "pub fn activate(db: &Pool, id: SubId) {\n    db.exec(\"UPDATE subscriptions SET status='active'\");\n    db.exec(\"UPDATE users SET subscription_status='active'\");\n}"));
    rows.extend(shell(
        "2026-10-11T08:20:00Z",
        "c8",
        "cargo test",
        "test result: FAILED. 1 failed",
    ));
    rows.extend(add_file("2026-10-11T08:21:00Z", "c9", "src/subscriptions/repo.rs", "pub fn activate(db: &Pool, id: SubId) {\n    db.exec(\"UPDATE subscriptions SET status='active' WHERE id = $1\");\n    db.exec(\"UPDATE users SET subscription_status='active'\");\n}"));
    let session = write_session(&root, rows);
    // The working tree as the agent left it, plus a derived lockfile and a manual tweak.
    for (file, body) in [
        (
            "Cargo.toml",
            "[package]\nname = \"app\"\n[dependencies]\nstripe = \"0.29\"\n",
        ),
        ("Cargo.lock", "# lock v2\nstripe 0.29\n"),
        (
            "src/signature.rs",
            "pub fn verify(sig: &str) -> bool { sig.len() > 8 }\n",
        ),
        ("src/webhooks/stripe.rs", &format!("{dedup}\n")),
        (
            "src/subscriptions/repo.rs",
            "pub fn activate(db: &Pool, id: SubId) {\n    db.exec(\"UPDATE subscriptions SET status='active' WHERE id = $1\");\n    db.exec(\"UPDATE users SET subscription_status='active'\");\n    manual_tweak();\n}\n",
        ),
        (
            "migrations/0007.sql",
            "CREATE TABLE webhook_events (event_id TEXT PRIMARY KEY);\n",
        ),
    ] {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    let r = review(&root);
    let work = session_work::build(&r, &session);
    (dir, r, session, work)
}

#[test]
fn hunks_are_attributed_by_textual_match_only() {
    let (_dir, r, _session, work) = webhook_repo();
    let spine = hunks::spine(&r, arr(&work["edits"]));
    let by_file = |file: &str| {
        spine
            .iter()
            .filter(|h| h["file"] == file)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        by_file("src/webhooks/stripe.rs")[0]["attribution"],
        "matched"
    );
    assert_eq!(by_file("src/signature.rs")[0]["attribution"], "matched");
    // Manual line added after the agent's write: partial, with the agent's edits as evidence.
    let repo = by_file("src/subscriptions/repo.rs");
    assert_eq!(repo[0]["attribution"], "partial");
    assert_eq!(
        arr(&repo[0]["evidence"]).len(),
        2,
        "both captured writes to the file contribute"
    );
    // Written via shell: shown, but no evidence.
    assert_eq!(by_file("migrations/0007.sql")[0]["attribution"], "none");
    assert!(arr(&by_file("migrations/0007.sql")[0]["evidence"]).is_empty());
    // Lockfiles are excluded from capture entirely; derived detection covers the name.
    assert!(by_file("Cargo.lock").is_empty());
    assert!(
        hunks::derived("Cargo.lock")
            && hunks::derived("web/package-lock.json")
            && !hunks::derived("src/lock.rs")
    );
    // Current-tree coordinates, stable ids, and a key that changes with content.
    let stripe = by_file("src/webhooks/stripe.rs")[0];
    assert_eq!(n(&stripe["start"]), 1);
    assert_eq!(n(&stripe["end"]), 7);
    let key = hunks::key(&spine);
    let mut edited = spine.clone();
    edited[0]["diff"] = json!("changed");
    assert_ne!(key, hunks::key(&edited));
    // No session: everything is `none`, nothing is assigned by proximity.
    let bare = hunks::spine(&r, &[]);
    assert!(bare.iter().all(|h| h["attribution"] == "none"));
}

#[test]
fn steps_group_by_intent_keep_fix_loops_together_and_report_gaps() {
    let (_dir, r, session, work) = webhook_repo();
    let list = steps::steps(&session, &work);
    let titles: Vec<&str> = list.iter().map(|st| s(&st["title"])).collect();
    assert_eq!(list.len(), 4, "{titles:?}");
    // Titles say what the step did: preamble and reason clauses stripped, capitalised.
    assert_eq!(titles[0], "Add the stripe dependency first");
    assert_eq!(
        titles[1],
        "Verify the Stripe-Signature header before parsing anything"
    );
    assert_eq!(titles[2], "Persist the event id");
    assert_eq!(titles[3], "Update the subscription repository");
    // Recorded reasons are verbatim statements; the dependency step has none.
    assert_eq!(list[0]["why"]["status"], "nearby-only");
    assert_eq!(list[1]["why"]["status"], "recorded");
    assert!(s(&list[1]["why"]["recorded"][0]["quote"]).contains("because unsigned requests"));
    assert!(s(&list[2]["why"]["recorded"][0]["quote"]).contains("idempotency boundary"));
    // Checks: passing test, build, then a failed test followed by a fix in the same step.
    assert_eq!(list[1]["checked"][0]["kind"], "test");
    assert_eq!(list[1]["checked"][0]["outcome"], "ok");
    assert_eq!(list[2]["checked"][0]["kind"], "build");
    assert_eq!(list[3]["checked"][0]["outcome"], "failed");
    assert_eq!(
        arr(&list[3]["edit_indices"]).len(),
        2,
        "edit → failed test → fix stays one step"
    );
    // The shell-written migration is a visible gap on the dedup step.
    assert!(
        arr(&list[2]["gaps"])
            .iter()
            .any(|g| s(g).contains("0007.sql"))
    );
    assert!(
        arr(&list[0]["gaps"])
            .iter()
            .any(|g| s(g).contains("No build or test"))
    );
    // Hunk ↔ step links go both ways through the edit indices.
    let spine = hunks::spine(&r, arr(&work["edits"]));
    let stripe = spine
        .iter()
        .find(|h| h["file"] == "src/webhooks/stripe.rs")
        .unwrap();
    assert_eq!(steps::for_hunk(&list, stripe), vec![2]);
    assert!(steps::hunks_for(&list[2], &spine).contains(&s(&stripe["id"]).to_owned()));
    let migration = spine
        .iter()
        .find(|h| h["file"] == "migrations/0007.sql")
        .unwrap();
    assert!(steps::for_hunk(&list, migration).is_empty());
}

fn question(packet: &Value, hunk_file: &str, status: &str, quote: &str, quote_id: &str) -> Value {
    let hunk = arr(&packet["hunks"])
        .iter()
        .find(|h| h["file"] == hunk_file)
        .unwrap();
    json!({"question":"What happens if Stripe sends the same event twice?","domain_why":"Providers deliver at least once.",
        "consequence":"high","consequence_note":"Double activation.","chosen":"Dedup by event id.",
        "said":{"status":status,"quote":quote,"quote_id":quote_id,"inferred_from":[]},
        "check":[{"hunk":hunk["id"],"line":n(&hunk["start"]),"note":"look for the early return"}],
        "expert_view":"","alternatives":[],"hunks":[hunk["id"]]})
}

#[test]
fn questions_are_validated_against_hunks_steps_and_verbatim_statements() {
    let (_dir, r, session, work) = webhook_repo();
    let spine = hunks::spine(&r, arr(&work["edits"]));
    let list = steps::steps(&session, &work);
    let packet = judgment::packet(&r, &spine, &list, std::slice::from_ref(&session));
    assert_eq!(arr(&packet["hunks"]).len(), 5);
    let dedup_statement = arr(&packet["statements"])
        .iter()
        .find(|st| s(&st["text"]).contains("idempotency"))
        .unwrap();
    let signature_statement = arr(&packet["statements"])
        .iter()
        .find(|st| s(&st["text"]).contains("unsigned"))
        .unwrap();
    let id = |st: &Value| s(&st["id"]).to_owned();
    // Verbatim quote from the step that produced the cited hunk: accepted.
    let good = question(
        &packet,
        "src/webhooks/stripe.rs",
        "recorded",
        "the unique index is the idempotency boundary",
        &id(dedup_statement),
    );
    judgment::validate_question(&good, &packet).unwrap();
    // Paraphrase: rejected.
    let paraphrased = question(
        &packet,
        "src/webhooks/stripe.rs",
        "recorded",
        "the unique index is our idempotency boundary",
        &id(dedup_statement),
    );
    assert!(judgment::validate_question(&paraphrased, &packet).is_err());
    // Real quote about a different change: rejected.
    let misattributed = question(
        &packet,
        "src/webhooks/stripe.rs",
        "recorded",
        "unsigned requests must be rejected",
        &id(signature_statement),
    );
    assert!(judgment::validate_question(&misattributed, &packet).is_err());
    // Unknown must carry no evidence; inferred must carry some.
    let mut unknown = question(&packet, "src/webhooks/stripe.rs", "unknown", "", "");
    judgment::validate_question(&unknown, &packet).unwrap();
    unknown["said"]["inferred_from"] = json!(["step-3"]);
    assert!(judgment::validate_question(&unknown, &packet).is_err());
    let mut inferred = question(&packet, "src/subscriptions/repo.rs", "inferred", "", "");
    assert!(judgment::validate_question(&inferred, &packet).is_err());
    inferred["said"]["inferred_from"] = json!(["step-4"]);
    judgment::validate_question(&inferred, &packet).unwrap();
    // Citations outside the diff, or check lines outside the hunk: rejected.
    let mut outside = good.clone();
    outside["hunks"] = json!(["h-nope-1-1"]);
    assert!(judgment::validate_question(&outside, &packet).is_err());
    let mut far = good.clone();
    far["check"][0]["line"] = json!(999);
    assert!(judgment::validate_question(&far, &packet).is_err());
    // A hunk with no step (shell-written migration) can only ever be unknown or inferred-from-hunk.
    let migration = question(&packet, "migrations/0007.sql", "unknown", "", "");
    judgment::validate_question(&migration, &packet).unwrap();
    // Scope key: tree edits and HEAD both invalidate; the session does too.
    let key = judgment::scope_key(&r, &spine, &session_work::snapshot_key(&session));
    assert_ne!(
        key,
        judgment::scope_key(&r, &spine[1..], &session_work::snapshot_key(&session))
    );
    assert_ne!(key, judgment::scope_key(&r, &spine, "other-session"));
    let mut moved = r.clone();
    moved["head"] = json!("deadbeef");
    assert_ne!(
        key,
        judgment::scope_key(&moved, &spine, &session_work::snapshot_key(&session))
    );
    // The packet never carries current code outside the diff.
    assert!(!packet.to_string().contains("lock v2"));
}

#[test]
fn verdicts_survive_regeneration_flag_stale_code_and_export_in_the_reviewers_voice() {
    let (_dir, r, session, work) = webhook_repo();
    let root = Path::new(s(&r["root"]));
    let spine = hunks::spine(&r, arr(&work["edits"]));
    let list = steps::steps(&session, &work);
    let packet = judgment::packet(&r, &spine, &list, std::slice::from_ref(&session));
    let mut q = question(&packet, "src/webhooks/stripe.rs", "unknown", "", "");
    q["question"] = json!("What happens if our handler crashes mid-way through an event?");
    q["expert_view"] = json!("One transaction covering dedup insert and side-effects.");
    q["key"] = json!(judgment::question_key(&q));
    let mut accepted = question(&packet, "src/signature.rs", "unknown", "", "");
    accepted["question"] = json!("Does the handler verify the request came from Stripe?");
    accepted["key"] = json!(judgment::question_key(&accepted));
    verdicts::set(
        root,
        &q,
        &spine,
        Some("push_back"),
        Some("need tx around insert+update"),
    )
    .unwrap();
    verdicts::set(root, &accepted, &spine, Some("accept"), None).unwrap();
    let saved = verdicts::load(root).unwrap();
    assert_eq!(saved.len(), 2);
    let v = &saved[s(&q["key"])];
    assert_eq!(v["state"], "push_back");
    assert!(!verdicts::stale(v, &q, &spine));
    // Regenerated wording with the same question and hunks keeps the verdict key.
    let mut regenerated = q.clone();
    regenerated["domain_why"] = json!("different prose");
    assert_eq!(
        judgment::question_key(&regenerated),
        judgment::question_key(&q)
    );
    // Editing the cited code marks the verdict stale rather than clearing it.
    let mut edited = spine.clone();
    for h in &mut edited {
        if h["file"] == "src/webhooks/stripe.rs" {
            h["diff"] = json!("@@ -0,0 +1,2 @@\n+changed\n+again");
        }
    }
    assert!(verdicts::stale(v, &q, &edited));
    assert!(!verdicts::stale(
        &saved[s(&accepted["key"])],
        &accepted,
        &edited
    ));
    let brief = json!({"questions":[q, accepted],"rejected":["bogus — cites a hunk outside the diff"],"head":r["head"]});
    let text = verdicts::export(
        &r,
        &brief,
        &list,
        &spine,
        &saved,
        "Add Stripe webhook handling",
    );
    let push = text.find("## Push back (1)").unwrap();
    let acc = text.find("## Accepted (1)").unwrap();
    assert!(
        push < acc,
        "push-backs lead so they can be pasted back to the agent"
    );
    assert!(text.contains("Reviewer: need tx around insert+update"));
    assert!(text.contains("Ask: One transaction"));
    assert!(text.contains("src/webhooks/stripe.rs:1-7"));
    assert!(text.contains("## Not in any question"));
    assert!(text.contains("migrations/0007.sql:1-1 (no captured evidence)"));
    assert!(text.contains("1 generated question(s) were rejected"));
    assert!(text.contains("## Agent's account"));
    assert!(text.contains("duplicate deliveries are ignored"));
    assert!(
        !text.contains("manual_tweak"),
        "export carries citations, not code"
    );
}
