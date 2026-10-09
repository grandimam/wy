# Architecture

wy is a Rust CLI and terminal workspace. Diffs, symbols and observable conversation messages supply evidence for engineering judgments.

```mermaid
flowchart LR
  H[Codex / Claude history] --> E[Normalized events]
  G[Git / baseline snapshot] --> D[Net changes]
  D --> R[Selective rules and evidence retrieval]
  E --> R
  R --> V[JSON Schema and citation validation]
  V --> DB[(Local SQLite)]
  DB --> CLI[CLI / terminal workspace]
  CLI --> A[Opt-in agent explanation]
  A --> V
```

- `main.rs` defines commands, argument validation, JSON output and error handling.
- `tui.rs` manages view history, question scope and background agent requests with progress and cancellation. Drafts keep their scope when an answer arrives. Answers are reused per file or symbol and check source freshness when reopened; terminal modes are restored on exit.
- `tui/explorer.rs` projects each review into a collapsible folder/file/symbol tree, with path filtering and session review marks that reset for changed content. `tui/document.rs` builds highlighted diffs and answers that lead with recorded justifications. `tui/render.rs` handles focus, responsive panes, input and mouse hit areas.
- The workspace's commit browser opens saved conversations without replacing the working-tree review or its reading history. `tui/layout.rs` manages draggable and keyboard-adjustable pane widths, saves preferences locally, and clamps displayed widths for smaller terminals.
- `history.rs` discovers project-matched Codex and Claude transcripts and normalizes observable events. Private reasoning and system records are omitted.
- `history/provenance.rs` classifies original messages, summaries and unknown captures at import, preserving available turn/message IDs and direct session lineage. `history/origins.rs` resolves explicit original references within repository and fork boundaries, pins saved source messages, and exposes missing or partial origins. It does not derive ancestry or intent from prose. Evidence identity includes captured content and provenance so resumed captures cannot collide merely by reusing a transcript line number.
- `history/edits.rs` extracts static Codex patches (including literal `tools.apply_patch` calls inside `exec`), completed file-change records and Claude Write/Edit/MultiEdit inputs. It never executes transcript code. `history/recent.rs` selects bounded recent edits, excludes known failures and resolves explanation scope against immutable saved sessions. These records remain separate from Git net changes.
- `repository.rs` reads Git revisions, working trees and imported diffs without applying patches or running repository code.
- `source.rs` uses tree-sitter for Rust, JavaScript, TypeScript and JSON outlines, and extracts Markdown headings. Other eligible text files use line anchors.
- `engine.rs` detects a bounded set of caching, retry, database, dependency and configuration choices. Evidence retrieval is lexical. Recorded rationale requires an explicit file-specific assistant statement.
- `reasoning.rs` prepares bounded evidence packets, calls the selected agent, validates citations and saves fresh assessments separately from recorded rationale.

- `agent.rs` runs explicit requests in temporary directories with repository tools and configuration disabled, output limits, cancellation and timeouts.
- `provider.rs` supports optional Ollama-compatible enrichment. Model responses cannot upgrade a decision to Recorded.
- `reflection.rs` manages nominated questions and imported retrospective assessments with source-freshness checks.
- `storage.rs` persists versioned JSON artifacts in local SQLite and writes an atomic review cache.
- `commits.rs` resolves commit revisions and retrieves pinned conversation snapshots through the local `commit_reviews` index. Lookup can populate the index by matching a review's base to the commit's parent and its captured source hashes to the committed snapshot; `wy link` adds an explicit association. Each link records its basis, and no association is inferred from a review's HEAD alone.
- `trace.rs` preserves original citations while showing current source and chronological transcript context.
- `security.rs` bounds and redacts inputs and validates source paths.

The workspace merges recent session files into its navigation tree without adding them to the Git diff. Wide terminals display code and explanation side by side; each scrolls independently. Source hit areas follow wrapped, scrolled text. An explanation of session code pins its captured edit and nearby conversation ahead of current-source context; answer reuse distinguishes that capture from the current diff of the same file.

A snapshot isolates changes since that working tree, including later commits. It does not prove authorship. Without a baseline, review compares a Git revision with the current working tree, including staged and non-ignored untracked files.

Decisions and source evidence carry original file hashes. Cached reads check freshness; changed source invalidates a decision rather than shifting its anchor. Captured transcript evidence remains historical. New model assessments retain their original evidence packet and flag changes during generation.

Recorded rationale requires an exact cited original assistant message. Summaries and unclassified history remain secondary or unknown even when written in the first person. Validation rejects inferred rationale supported only by those sources. Older saved answers are conservatively relabeled on display without rewriting immutable captures. Structural checks cannot establish whether a quote semantically justifies a particular code choice.

Detection is selective and capped at twelve decisions. It cannot discover every architectural choice. Lexical matches and structurally valid citations do not establish semantic correctness. Local artifacts are unencrypted and have no automatic deletion policy.
