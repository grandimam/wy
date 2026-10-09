# Using wy

Install directly from GitHub with `cargo install --git https://github.com/grandimam/wy --locked wy-code`. This builds from source and requires Rust 1.88+, Git and a C toolchain. From a checkout, use `cargo install --path . --locked` to install or `cargo run --` in place of `wy` to run it.

## Prebuilt installer

The release workflow is configured to provide binaries for Apple Silicon and Intel macOS, and ARM64 and x64 Linux with glibc. **The first binary release must be published before this command is available:**

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/grandimam/wy/releases/latest/download/wy-code-installer.sh | sh
```

The installer selects your platform, checks the archive's checksum and installs `wy` into `~/.local/bin`. It updates supported shell profiles to put that directory on your PATH; open a new terminal afterward. Repeating the command installs the latest release. Git is still required to review repositories; Rust is only needed for source builds. Windows binaries are not currently configured.

To choose another directory without updating shell profiles, download the installer and run it with `WY_CODE_UNMANAGED_INSTALL`:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/grandimam/wy/releases/latest/download/wy-code-installer.sh -o /tmp/wy-installer.sh
WY_CODE_UNMANAGED_INSTALL="$HOME/bin" sh /tmp/wy-installer.sh
```

See [release setup](releasing.md) for publishing and checking the first release.

## Terminal workspace

Run `wy --repo /path/to/repository` in an interactive terminal. Startup performs an offline review of current changes and matching project history.

The file tree combines working-tree changes with recent recorded edits, including files already committed. There is one **Changes** view: it prefers the current Git diff, falling back to the latest captured edit when a file has no current diff. A source label makes the distinction explicit. Recorded edits can differ from current files and are not reconstructed complete files. Coverage includes the latest edit per file from up to three coding sessions in the last seven days, capped at 50 files.

Select a file or changed function to see **Agent notes** immediately: excerpts of the original conversation, with sources you can open. No model call is needed. Notes are linked by file references and nearby edits within a user turn. Gap hints identify missing signals in those excerpts, such as an explicit reason or alternatives; they are not a complete assessment of the agent's reasoning. On terminals at least 110 columns wide, notes sit to the right of the changes. On smaller terminals, Tab switches panes.

Press **e** or click **Enrich explanation** to connect the code and notes with a new, cited assessment. Keep navigating while it runs. Files show **working**, **queued**, or **ready**; return to a file to read its enrichment. Reading positions are retained while browsing, and completed answers are restored when wy restarts. **o Agent notes** returns to the original excerpts; **v Enriched** opens the saved assessment without a new request.

| Key | Action |
| --- | --- |
| Up / Down or `j` / `k` | Preview the selected file or function; scroll the focused pane |
| `e` (also `w`) | Enrich the selected change; reuse a completed answer when available |
| `d` | Focus Changes |
| `o` / `v` | Original agent notes / saved enrichment |
| `s`, then Up/Down and Enter | Select and open a source; Escape returns to reading |
| `1`–`9` | Open a numbered code or conversation source |
| `i` | Ask a question about the displayed notes or answer |
| `R` | Request updated enrichment |
| Tab | Switch between files, code and explanation |
| Left / Right or `h` / `l` | Collapse / expand the tree; pan code |
| Space | Toggle a folder or a file's changed-symbol outline |
| Enter | Focus the file's changes, open a selected source, or toggle a folder |
| `f` | Filter file paths |
| `g` | Browse the 50 most recent commits; Up/Down selects and Enter opens saved conversations |
| `[` / `]` | Move the file divider left/right when files are focused; otherwise move the code/notes divider, when visible |
| `r` | Refresh changes and captured history offline |
| Escape | Return to the previous view or clear a file filter |
| `x` | Cancel running and queued enrichments |
| `?` / F1 | Help, settings and additional commands |
| `q` / Ctrl+Q / Ctrl+C | Quit and cancel unfinished requests |

Click files, sources or view tabs to navigate. The mouse wheel scrolls the pane under the pointer. PageUp/PageDown and Home/End navigate longer content. Below 90 columns, panes use the full width. Input supports paste and Ctrl+U to clear. A result arriving in the background will not switch you to another file, close a source, or interrupt a question draft.

Drag either vertical divider to resize the file sidebar or the code/notes panes. Minimum widths keep both sides readable. Pane sizes are saved locally for this repository and restored when wy restarts; smaller terminals temporarily clamp them. Use `/layout reset` to restore the default sizes.

Press **g** or click **g Commits** in the header to browse recent commits. Select a commit and press Enter, or click its row, to read its captured conversations. `/commit <hash>` opens any commit directly, including abbreviated hashes and Git revisions. Automatic matching uses the same saved review base and source hashes as shell lookup. If no capture matches, `/link <hash> [review-id]` explicitly attaches a saved review (the latest by default). Escape returns to the previous view. Commit browsing stays offline and does not replace the current working-tree review.

**Enrich makes new model calls**: one selects relevant context and another writes a cited explanation. Captured notes are included as evidence; inferred reasons are a new assessment. Requests may consume your signed-in account's usage. Explicit requests for other files queue in the background (up to eight waiting requests). Follow-ups retain the original file, function or captured edit, including while reading a source about another file. Code browsing, notes, filtering and source navigation stay offline.

`p` reopens the last saved explanation, including one generated by `wy reason` or `wy why` in the shell. Source freshness is checked when an answer is opened or revisited; `R` explicitly updates it.

Use `/agent codex|claude` to choose the answering agent and `/source both|codex|claude|none` for the conversation history. Additional commands are `/why FILE:SYMBOL QUESTION`, `/ask QUESTION`, `/reason QUESTION` (all changes), `/evidence NUMBER`, `/commits`, `/commit HASH`, `/link HASH [REVIEW-ID]`, `/layout reset` and `/cancel`. Offline detected choices remain available through the shell's `wy decisions` and `wy explain` commands.

The optional `m` shortcut marks a file reviewed for this session. Refreshing clears marks for changed content. Marks track your progress, not correctness or test results. `b` toggles the file sidebar.

## Review changes

```bash
wy snapshot --json
# Run your coding agent.
wy review --baseline <snapshot-id>
wy decisions
wy explain 1 --show-code
wy evidence 1 1
wy gaps
wy ask 1 'What assumptions should I verify?'
```

Without a baseline, review compares HEAD with the current working tree, including staged and non-ignored untracked files. A baseline separates later changes from pre-existing edits; it cannot establish authorship. Use only one of `--baseline`, `--base REV` and `--diff PATCH`. Imported patches are checked against source and never applied.

Commands accept `--repo PATH` and `--json`. The workspace requires a terminal; use a subcommand for scripts. Saved source hashes are checked before cached decisions are read or investigated.

## Shell command reference

| Command | Purpose |
| --- | --- |
| `wy review` | Review current changes and matching history offline. |
| `wy sessions --commit REV` | Find saved conversations associated with a commit. |
| `wy session --commit REV` | Read the captured conversation events for a commit. |
| `wy link REV --review ID` | Explicitly associate a saved review with a commit. |
| `wy why worker.rs:batch` | Investigate a function through your agent CLI. |
| `wy reasoning-evidence 1 --id <explanation-id>` | Inspect the saved source behind an explanation. |
| `wy decisions` | List detected decisions and review origin. |
| `wy explain 1` | Inspect a decision's rationale and citations. |
| `wy evidence 1 2` | Open the second citation for the first decision. |
| `wy gaps` | Find unanswered questions, unexplained choices and stale findings. |
| `wy decisions --json` | Get structured output for scripts. |

In-app `/ask` calls the selected agent CLI. The shell command `wy ask TARGET 'QUESTION'` works offline unless you add `--model`.

## Project history

```bash
wy sessions
wy review --source claude
wy review --source none
wy review --session codex:<id>
wy session --id codex:<id> --event event-42 --json
```

History discovery uses `$CODEX_HOME/sessions` and `archived_sessions` (default `~/.codex`) and `$CLAUDE_CONFIG_DIR/projects` (default `~/.claude`), plus equivalent repository-local layouts. Only sessions whose recorded working directory belongs to this Git repository are accepted. Repeat `--session` for multiple IDs or explicit JSONL paths.

Observable text, tool calls and results are retained; private reasoning and system records are omitted. History is bounded to 20 sessions and 40 MB total, with a 20 MB per-transcript limit. Saved session excerpts are contextual evidence, not authenticated authorship.

### Find conversations by commit

Capture a review before committing. Once the changes are committed, look up their saved conversations with a full hash, an unambiguous short hash, or a Git revision:

```bash
wy review
# Commit the reviewed changes with Git.
wy sessions --commit HEAD
wy session --commit <commit-hash>
wy session --commit <commit-hash> --id codex:<session-id> --event event-42 --json
```

Lookup automatically records a local association when a saved review's comparison base and HEAD match the commit's parent, and every captured source-file hash matches the committed source snapshot. It also supports the first commit in a repository. It searches saved reviews, independently of the latest review or current checkout. The automatic match covers the source files wy captures; it does not establish authorship or prove every conversation event caused the change. The output labels these associations `snapshot-match`.

Partial commits, edited source after review, merges, baseline/imported-patch reviews, or missing prior captures may not match automatically. Attach an existing saved review explicitly in those cases:

```bash
wy link <commit-hash>                     # Attach the latest saved review.
wy link <commit-hash> --review <review-id>
wy sessions --commit <commit-hash> --source claude --json
```

Explicit associations are labeled `explicit`. Multiple reviews can be attached to a commit; linking the same review again is safe. References pin the captured session snapshots, so later reviews, changed transcripts, or deleted original transcript files do not replace the saved excerpts. Different captures of the same session remain distinguishable by `storage_key`; identical captures are listed once with their review IDs. Missing saved evidence produces an error.

These links and excerpts stay in the local `.wy/wy.sqlite3` database. They are discovered during lookup, without a Git hook or background process. A clone on another machine does not receive them. A rewritten commit has a new hash and may need an explicit association. A review must contain saved conversation history for `wy link` to attach it.

### Summaries and original turns

The importer classifies captured messages as original turns, compacted summaries, unknown provenance, or tool records. Compacted summaries display **Secondary evidence · Compacted summary** in agent notes, sources, and commit conversations. A compaction marker without readable summary text still displays the missing context explicitly.

In a summary's source view or commit conversation, click **Open original turn**, or press **s**, select the source, and press Enter. Escape returns to the summary. The source opens the redacted saved message, pinned by snapshot, event identity and content hash, even if the raw transcript has been deleted. Retained original context has a distinct link label: it provides context, without claiming that every summary assertion is supported by that message.

Resolution uses explicit message/turn references and saved snapshots, with a fallback to a repository-matched local transcript. It never matches originals by similar wording or guesses an earlier user request. Resuming the same session can resolve references against earlier captures. Cross-session references currently require a directly recorded parent session and an inclusive turn cutoff; messages after that cutoff are excluded. Missing boundaries, indirect or unknown ancestry, conflicting captures, and missing originals remain unavailable. This is deliberately conservative when a provider omits provenance metadata.

**Original turn unavailable** appears explicitly when no original can be verified. Partial resolution says that some originals are unavailable. A summary or unclassified capture cannot supply a Recorded quote, and a judgment relying only on those sources must leave original rationale unknown. Code-based assessments remain possible when independently supported. An original assistant quote records what the assistant said; it does not authenticate authorship or prove the reason correct.

Older snapshots without provenance remain readable as **Unknown provenance**. Refresh with **r** or `wy review` to classify an available transcript in a new capture; pinned historical captures are preserved. Old saved answers lose unsupported Recorded/Inferred labels and display a warning about their earlier prose. Each lookup is bounded to 32 references, 20 saved captures, and 40 MB of captured data; exceeding a limit is reported as unavailable context. Provider format coverage is described in [Codex ingestion support](codex-formats.md).

## Agent explanations

Install and sign in to Codex or Claude Code, then request an explanation explicitly:

```bash
wy reason --agent codex
wy reason --agent claude --file src/service.rs
wy why src/reasoning.rs:resolve_target --question 'Would a simpler approach work?'
wy why src/storage.rs:27
wy reasoning-evidence 1 --id <reasoning-id> --json
```

Symbol targets use tree-sitter outlines; line targets work for any eligible source file. No automatically detected decision is required. The agent receives bounded, redacted code, diff and observable history from a temporary directory with repository tools and configuration disabled. Output schemas, citations and source freshness are validated before saving. These are fresh assessments, not recovered private reasoning.

**Recorded** means an explicit relevant assistant justification was found. **Inferred** means an evidence-based hypothesis. **Unexplained** means motivation was not established. A recorded statement can still be wrong.

## Reflection workflow

An existing coding conversation can supply a retrospective assessment:

```bash
wy focus src/storage.rs:27 'Why store artifacts in SQLite?' --evidence src/storage.rs:41
wy reflection-request --json
# Answer the request using its response_schema and write JSON locally.
wy record-reflection /tmp/wy-reflection.json
wy explain 1
```

Request identity, membership, citations and source freshness are checked. Assessments do not replace original explanations or upgrade them to Recorded. Agent identity and original-conversation context are self-reported.

## Optional model analysis

The direct provider supports Ollama-compatible chat endpoints:

```bash
export WY_MODEL='your-installed-model'
export WY_MODEL_ENDPOINT='http://127.0.0.1:11434/api/chat'
wy review --model
wy ask 1 'What assumptions should I verify?' --model
```

Remote endpoints require HTTPS and `WY_ALLOW_REMOTE=1`. Optional `WY_PROVIDER_API_KEY` supplies a bearer token. Configuration comes from process environment variables. Calls require `--model`; malformed results preserve conservative offline analysis. Redaction and citation validation cannot prove semantic correctness.

Local artifacts live in `.wy/`. They contain source snapshots and history excerpts, are unencrypted, and should be excluded from version control.
