# Evaluation

Run `cargo test --locked` for Rust regression checks and `cargo check --locked` to check the library and binary. Tests use temporary repositories and terminal rendering doubles; they do not call real models.

Workspace checks cover tree expansion, filtering and diff previews, reuse of answers for the selected change, recorded-reason ordering, focus and scroll restoration, scoped questions during background completion, citation ownership across navigation, review-mark invalidation, reopened explanation freshness, mouse input, compact layouts, cancellation and worker failures. Binary checks confirm that only the workspace is offered and removed subcommands are rejected. Optional terminal-buffer previews can be written with `WY_TUI_PREVIEW_DIR=/tmp/wy-previews cargo test --locked tui::tests::renders_review_diff_explanation_and_empty_states`.

Automated checks establish implementation behavior, not real-world explanation accuracy. Human evaluation should inspect the original change and observable transcript, score explanation usefulness, check whether citations support each claim, and distinguish recorded statements from hypotheses.

Session-code checks cover committed files with a clean Git diff, captured code surviving later deletion, static wrapped patches, Claude writes and edits, failed tool calls, private-path exclusion, redaction, recency bounds and Codex event IDs. Workspace checks also cover separate answers for a diff and its historical edit, code beside prose, clickable and keyboard-selected sources, and narrow-terminal access to both panes.

## Plan and Steps

`tests/judgment.rs` covers the hunk spine (matched / partial / none, derived files,
current-tree coordinates, stable ids, no session ⇒ nothing attributed), step
segmentation (announce + disjoint files splits, decision records split, edit → failed
test → fix stays together, validation outcomes, shell-write gaps, hunk ↔ step links),
question validation (verbatim quotes only, quotes from the step that produced the
cited code only, unknown carries no evidence, inferred needs some, citations and
check lines inside the diff, scope-key invalidation by tree, HEAD and session) and
verdicts (persist across regeneration, stale by hunk hash, export ordering and
content). `src/tui/plan_tests.rs` covers the dirty-tree default with every plan row openable
and nothing running, progress marks after returning, the gap and tests details, two
tabs at 40/80/140 columns, Steps boxes with Why / Check / Gap / Plan rows and the
transcript drill-down, worry details with hunk markers and step cross-links, y/n/m
and notes re-rendering in place, stale flags on both screens, export preview,
asking as a row with mismatched-scope and failed results, and the clean-tree state.

**Does wy actually build judgment?** That is not established by these tests.
Evaluate it with representative dirty trees in domains the reviewer does not know:

1. Independently have a domain expert list the questions that must be answered
   before accepting the diff. Do not show them wy's output first.
2. Give a developer with no background in the domain the diff and wy. Let them form
   the questions, work through them, and record verdicts.
3. Measure: how many of the expert's questions wy surfaced (coverage); how many the
   newcomer could then answer correctly (comprehension); how many wy answered
   **wrongly with confidence**—a `recorded` or `inferred` status, an expert view or
   a consequence note that the expert rejects. The last number must be near zero;
   it matters more than coverage.
4. Track whether the newcomer, on a later diff in the same domain, asks the
   questions before running wy. That is the outcome.

## Decision visibility milestone

Request-scope regression checks cover overlapping files with separate rationale,
normalized selection/cache identity, invalid request ranges, selection reset across
snapshots, explicit request-to-overview navigation, empty versus ungenerated briefs,
and failure/retry presentation. The stub-CLI integration also verifies that discovery
rebuilds selected requests from the pinned snapshot, excludes another request's code
and text even if the caller supplies whole-session edits, and rejects out-of-scope
citations. These checks do not establish real-model explanation quality.

Decision tests cover multi-record parsing, cross-file grouping without collapsing
conflicting rationale, clean-tree/missing-history states, in-scope diff citations,
exact recorded-reason quotes, secondary-history rejection, scope-key invalidation,
and persisted brief reuse. TUI checks exercise overview → detail → code evidence
→ back, mouse input at 40/80/140 columns, and background completion without
interrupting a deep dive or draft. An isolated stub CLI test exercises the actual
request/validation/persistence path, including cancellation, stale source and
invalid-response rejection. It also exercises session-only discovery with current
code containing a sentinel that must never appear in the transmitted packet,
and repeats the request after deleting the current file and original transcript.
It does not call a real model.

Session-work tests additionally verify:

- Latest selection uses source dates, including timezone offsets, not file mtimes.
- Startup selects the latest capture; explicit session selection survives refresh.
- Overlapping edits in different sessions never share decision evidence.
- A clean Git tree, commits, later edits and file deletion do not erase session work
  or invalidate its snapshot-keyed brief.
- New captured session content does invalidate that brief.
- Failed/unsafe edits are excluded, repeated edits retain source order, and the
  implementation flow remains pageable without discarding later steps.
- Current-code comparison is opt-in and conservative for partial, unconfirmed or
  nonfinal writes. No patches are applied to current code to infer historical bases.
- Decisions / Sessions navigation works at 40/80/140 columns; old sessions require
  the picker, and unassigned working-tree changes remain a separate view.

The **two-minute outcome is not established by these tests**. Evaluate it with
representative captured sessions (including cross-file choices, multiple decisions
in one file, overlapping sessions, committed work, absent transcripts and partial
historical records):

1. Have independent engineers identify the consequential choices and supporting
   evidence before showing the generated brief. Do not treat the implementing
   agent's explanation as ground truth.
2. Give a developer unfamiliar with the change two minutes starting at the decision
   overview. Ask them to explain the most consequential choices, one trade-off each,
   and which rationales are recorded versus inferred or unknown.
3. Measure time, consequential-choice coverage, comprehension and unsupported
   claims. Track whether they needed to reconstruct decisions from raw history.
4. Compare against diff-only review and the previous file/history-first experience.
   Record evidence omissions and disagreements, not just task completion.

Useful measurements include explanation relevance, unsupported claims, uncertainty honesty, stale-source handling and provider-reported token usage. Compare model output with the actual evidence packet, and verify cost against provider accounting. No real-model quality or cost results are claimed.
