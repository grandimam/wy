# Evaluation

Run `cargo test --locked` for Rust regression checks and `cargo check --locked` to check the library and binary. Tests use temporary repositories and terminal rendering doubles; they do not call real models.

Workspace checks cover tree expansion, filtering and diff previews, reuse of answers for the selected change, recorded-reason ordering, focus and scroll restoration, scoped questions during background completion, citation ownership across navigation, review-mark invalidation, reopened explanation freshness, mouse input, compact layouts, cancellation and worker failures. Binary checks confirm that only the workspace is offered and removed subcommands are rejected. Optional terminal-buffer previews can be written with `WY_TUI_PREVIEW_DIR=/tmp/wy-previews cargo test --locked tui::tests::renders_review_diff_explanation_and_empty_states`.

Automated checks establish implementation behavior, not real-world explanation accuracy. Human evaluation should inspect the original change and observable transcript, score explanation usefulness, check whether citations support each claim, and distinguish recorded statements from hypotheses.

Session-code checks cover committed files with a clean Git diff, captured code surviving later deletion, static wrapped patches, Claude writes and edits, failed tool calls, private-path exclusion, redaction, recency bounds and Codex event IDs. Workspace checks also cover separate answers for a diff and its historical edit, code beside prose, clickable and keyboard-selected sources, and narrow-terminal access to both panes.

Useful measurements include explanation relevance, unsupported claims, uncertainty honesty, stale-source handling and provider-reported token usage. Compare model output with the actual evidence packet, and verify cost against provider accounting. No real-model quality or cost results are claimed.
