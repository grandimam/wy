# Evaluation

Run `cargo test --locked` for Rust regression checks and `cargo check --locked` to check the library and binary. Tests use temporary repositories and terminal rendering doubles; they do not call real models.

Workspace checks cover tree expansion, filtering and diff previews, reuse of answers for the selected change, recorded-reason ordering, focus and scroll restoration, scoped questions during background completion, citation ownership across navigation, review-mark invalidation, reopened explanation freshness, mouse input, compact layouts, cancellation and worker failures. Binary checks confirm that only the workspace is offered and removed subcommands are rejected. Optional terminal-buffer previews can be written with `WY_TUI_PREVIEW_DIR=/tmp/wy-previews cargo test --locked tui::tests::renders_review_diff_explanation_and_empty_states`.

Automated checks establish implementation behavior, not real-world explanation accuracy. Human evaluation should inspect the original change and observable transcript, score explanation usefulness, check whether citations support each claim, and distinguish recorded statements from hypotheses.

Session-code checks cover committed files with a clean Git diff, captured code surviving later deletion, static wrapped patches, Claude writes and edits, failed tool calls, private-path exclusion, redaction, recency bounds and Codex event IDs. Workspace checks also cover separate answers for a diff and its historical edit, code beside prose, clickable and keyboard-selected sources, and narrow-terminal access to both panes.

## Decision visibility milestone

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
