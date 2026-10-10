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

Run `wy` in a repository, or `wy --repo /path/to/repository`, in an interactive terminal. Startup performs an offline review of current changes and matching project history.

The screen has two areas: the file tree on the left and one reader on the right. wy draws no boxes or backgrounds and uses your terminal's own colours, so it follows your theme and works on light backgrounds.

### The file tree

The tree combines working-tree changes with recent recorded edits, including files already committed. Each file shows its line counts, or **· recorded** when it has no current diff. Coverage includes the latest edit per file from up to three coding sessions in the last seven days, capped at 50 files. Press `f` to filter paths, Space to expand a file's changed symbols, and `m` to mark a file reviewed for this session.

### Main-screen navigation

The reader has three always-visible tabs for a selected file:

- **Changes** (`o`): current diff and linked agent messages. Unmatched changes show code first; file-mention conversation is collapsed below it.
- **Explanation** (`v`): a saved AI assessment, or an **Ask AI to explain this change** button. Opening the tab never makes a model call. Click the button, or select it and press Enter; `e` performs the same action. A request can consume your configured CLI account's usage.
- **History** (`t`): dated turns for this file. Click a tab to switch, use its letter shortcut, or cycle with **Ctrl+Left / Ctrl+Right** (if your terminal forwards these keys). Tabs have equal-width click targets and a filled selected state. Returning to a cached History tab preserves its page, scroll and expanded notes. Returning to Changes reuses its captured attribution rather than rescanning sessions. Refreshing clears these caches.

The file tree separates **Current changes** from **Earlier sessions** when both exist; both groups can be collapsed. Explanation status (working, queued, ready) belongs to the selected file's reader title, not to every file row.

All history surfaces—including turns, sessions, timelines, commit conversations, and cited sources—use the same presentation: original messages remain readable; **Agent notes**, summaries, tool activity, and metadata are collapsed. Expand a row by clicking it or selecting it and pressing Enter. Tool/model names, identifiers, and verification details belong inside details, not above every message. Captured working notes do not imply a missing transcript and do not require re-import merely because they cannot prove intent.

### The reader

The reader leads with **YOUR REQUEST** in a padded, bordered Ratatui card, expanded by default, followed by a separate code panel. Request cards keep a readable width and wrap with the terminal; code panels use the available width without wrapping source lines. A shared request appears once above related changes. Confirmations such as “yes, do it” use an earlier substantive request from the same session when available, explicitly labeled as context rather than a proven causal link. Short instructions such as “Add caching” remain requests in their own right. When no original request can be established, wy says so instead of promoting a summary or inventing a requirement.

Current Git diffs start expanded under **Code changes**; historical file writes start closed under **Code saved in this session**, and historical replacements under **Before / after**. Requests can also be collapsed. Click a disclosure or select it and press Enter to toggle it; `z` collapses/expands all code blocks in the current view. Supporting **Agent response**, **Agent notes**, and **Technical details** stay closed until requested. **View original chat** opens the saved chat around the edit—it does not start a new AI conversation. Raw patches and session metadata live inside **Technical details**, not beside the main reading controls.

Select a file to read its changes in file order, with nearby captured conversation above matching edits. Historical edit dates and the review capture date are shown separately:

```
 1  I'll keep responses in memory.  gpt-5.4
┃ Repeated reads can then reuse a response without another network call.
┃ You asked: "Avoid fetching the same response repeatedly."

@@ line 10 · fn get()  +2 −1  turn ›
 fn get() {
-    fetch()
+    cache.get(key)
+        .unwrap_or_else(fetch)
 }

@@ line 42 · fn refresh()  +1 −0  · no matching edit record
+    log::debug!("hit");
```

- **Nearby conversation** starts with a numbered badge and the message preceding a recorded edit, with tool, model, session and date. It is not automatically a verified explanation. Press `w` to collapse these blocks.
- **Matching is textual, not causal.** wy compares changed-line text with supported recorded edits, ignoring punctuation-only lines. Several hunks can share the same nearby context. A match is not proof of authorship or intent.
- **No matching edit record** means no captured edit candidate overlaps that hunk. **Partial text overlap** means only some text matches; it does not prove who changed the rest or when. Use `/coverage` for capture gaps.
- **View original chat** opens the surrounding saved conversation: your request, the agent's messages, and the code excerpt. Esc returns.
- Files with no current Git diff show historical replacement excerpts instead, with separate **Before / After** blocks. Their file positions are unknown; snippet-relative patch headers are not presented as real file line numbers. The main view limits the displayed excerpt to 80 lines across both sides; open **View original chat** or **Technical details** for the captured input.
- Current Git diffs show old/new line-number gutters. Code does not wrap: use Left/Right or `h`/`l` to pan, while prose remains wrapped and stationary. A `›` at the right edge marks hidden code. Patch transport markers, including `No newline at end of file`, stay in the raw patch inside **Technical details**, rather than interrupting the readable view.
- A yellow **CONTEXT COMPACTED** banner indicates earlier context may have been replaced by a summary. The exact retained context varies; summaries are secondary evidence, not proof of original intent.
- When no hunk matches any edit but the conversation mentions the file, a **Related conversation** block appears first, labelled as matched by file mentions rather than by an edit.

Readable rationale is shown separately as tentative context. Hidden/encrypted reasoning is not recoverable. A structured decision record is a self-reported explanation, not independent verification.

### Enrichment

Press **e** to connect the code and reasons with a new, cited assessment from your Codex or Claude CLI. Keep navigating while it runs. Files show **working**, **queued**, or **ready**; return to a file to read its answer. Completed answers are restored when wy restarts. The **Explanation** tab is always present; `v` opens it and `o` returns to **Changes**. `p` reopens the last saved answer. `R` requests an updated one.

| Key | Action |
| --- | --- |
| Up / Down or `j` / `k` | Select a file; scroll the reader |
| Tab / Shift+Tab | Move between the file tree and the reader / back to the tree |
| Enter | In the tree: read the file. In the reader: select changes; Enter again opens the turn |
| `w` | Collapse reasons to headlines, or expand them |
| `e` | Enrich the selected file; reuse a completed answer when available |
| `o` / `v` / `t` | Changes / Explanation / History |
| `s`, then Up/Down and Enter | Select and open a source of an answer; Escape returns to reading |
| `1`–`9` | Open a numbered source of an answer |
| `i` | Ask a follow-up about the displayed answer |
| `R` / `p` | Request an updated answer / reopen the last saved answer |
| Left / Right or `h` / `l` | Collapse / expand the tree; pan long lines |
| Space | Toggle a folder or a file's changed-symbol outline |
| `f` | Filter file paths |
| `g` | Browse the 50 most recent commits; Up/Down selects and Enter opens saved conversations |
| `[` / `]` | Resize the file tree |
| `r` | Refresh changes and captured history offline |
| `m` / `b` | Mark the file reviewed / toggle the file tree |
| Escape | Return to the previous view or clear a file filter |
| `x` | Cancel running and queued enrichments |
| `?` / F1 | Help, settings and commands |
| `q` / Ctrl+Q / Ctrl+C | Quit and cancel unfinished requests |

Click files, change headers, sources or the Enriched tab to navigate. The mouse wheel scrolls the area under the pointer. PageUp/PageDown and Home/End navigate longer content. Below 88 columns, the tree and the reader take turns using the full width. Input supports paste and Ctrl+U to clear. A result arriving in the background will not switch you to another file, close a source, or interrupt a question draft.

Drag the vertical divider or use `[` / `]` to resize the file tree. The width is saved locally for this repository and restored when wy restarts; `/layout reset` restores the default.

Press **g** to browse recent commits. Select a commit and press Enter, or click its row, to read its captured conversations. `/commit <hash>` opens any commit directly, including abbreviated hashes and Git revisions. Automatic matching compares saved review bases and source hashes; see [Find conversations by commit](#find-conversations-by-commit). If no capture matches, `/link <hash> [review-id]` explicitly attaches a saved review (the latest by default). Escape returns to the previous view. Commit browsing stays offline and does not replace the current working-tree review.

**Enrich makes new model calls**: one selects relevant context and another writes a cited explanation. Captured reasons are included as evidence; inferred reasons are a new assessment. Requests may consume your signed-in account's usage. Explicit requests for other files queue in the background (up to eight waiting requests). Follow-ups retain the original file, function or captured edit, including while reading a source about another file. Browsing, filtering and source navigation stay offline.

Use `/agent codex|claude` to choose the answering agent and `/source all|codex|claude|pi|opencode|none` for the conversation history (`both` is an alias for all). Additional commands are `/why FILE:SYMBOL QUESTION`, `/ask QUESTION`, `/reason QUESTION` (all changes), `/evidence NUMBER`, `/commits`, `/commit HASH`, `/link HASH [REVIEW-ID]`, `/layout reset` and `/cancel`.

## Everything happens in the app

wy has no subcommands. `wy` opens the workspace for the current repository; `--repo PATH` opens another one, and `--version` prints the version. The workspace needs an interactive terminal.

wy keeps reviews, source snapshots and redacted conversation excerpts in `.wy/` inside the repository. Add `.wy/` to your `.gitignore` before sharing the repository; it is local data, not encrypted, and redaction is best-effort.

Startup compares HEAD with the current working tree, including staged and non-ignored untracked files. wy cannot tell edits made before the agent session apart from the agent's own edits, so attribution remains unknown. Press **r** to refresh after more changes.

## Project history

History discovery uses `$CODEX_HOME/sessions` and `archived_sessions` (default `~/.codex`) and `$CLAUDE_CONFIG_DIR/projects` (default `~/.claude`), plus equivalent repository-local layouts. Only sessions whose recorded working directory belongs to this Git repository are accepted. Use `/source all|codex|claude|pi|opencode|none` to choose which history is read (`both` remains an alias for all four). pi and OpenCode discovery, available rationale capture, and a decision-record instruction template are described in [Decision records](decision-records.md).

Observable text, tool calls/results, readable Claude/pi thinking, OpenCode reasoning and Codex reasoning summaries are retained. Rationale is tentative; signatures, encrypted/redacted payloads, system records and Codex analysis-channel messages are omitted. History is bounded to 20 sessions and 40 MB total, with a 20 MB per-transcript limit. Saved session excerpts are contextual evidence, not authenticated authorship.

### Coverage, dated sessions, and decisions

- `/coverage` shows per-agent discovery/capture counts, budget exclusions, unreadable sources, scope/identity mismatches, and current hunks without matching edits. Invalid candidates with unknown scope are reported separately; one inaccessible database does not stop other agents from being captured.
- `/sessions` lists separate captured sessions with tool, model(s), start/earliest-event date, and last captured event date. Enter opens the selected session. The transcript is paginated at 20 events per page, starting with the latest page. Earlier captured events remain accessible through **Previous / Next page** controls or Alt+Left/Right if forwarded by your terminal.
- `/timeline [FILE:SYMBOL]` displays related user-bounded turns across tools, oldest first. `/decisions [FILE:SYMBOL]` adds parsed decision records, requirements, captured test outcomes and proposed reviewer checks. Omit the argument to use the selected file. Symbol matching is explicit text/record matching, not semantic causality. Views paginate at 20 events per page, with newest conversations first and messages within each conversation in recorded order. Large turns continue onto additional pages; pagination does not discard older captured turns. Decision/test overview lists are bounded to eight records each; the underlying captured events remain pageable.
- Dates use absolute UTC plus relative age. Unknown dates stay unknown. The review capture timestamp describes when wy read history, not when an old edit happened. Later file edits are flagged as possibly making earlier explanations inapplicable, not automatically treated as a confirmed reversal.
- `/setup` previews optional public decision-record instructions. `/setup save` writes `.wy/decision-instructions.md` without changing `AGENTS.md` or any agent configuration. Review and copy the instructions yourself. Existing files are not overwritten.
- `/export` previews a bounded offline review brief; `/export save` writes that exact preview to a new private `.wy/review-brief-<digest>.md` file. Nothing is uploaded. The brief excludes raw transcripts/tool outputs and tentative rationale, and uses best-effort redaction. Review for sensitive content before sharing. It describes the dated capture, not a live review. Repeated saves of the same brief do not overwrite existing files.

See [Decision records](decision-records.md) for the structured format and limitations.

## Find conversations by commit

wy saves a review of your changes each time it opens or refreshes. After committing those changes, press **g** and choose the commit, or enter `/commit REV` with a full hash, an unambiguous short hash, or a Git revision.

Lookup automatically records a local association when a saved review's comparison base and HEAD match the commit's parent, and every captured source-file hash matches the committed source snapshot. It also supports the first commit in a repository. It searches saved reviews, independently of the latest review or current checkout. The automatic match covers the source files wy captures; it does not establish authorship or prove every conversation event caused the change. These associations are labelled as matching the review base and source snapshot.

Partial commits, source edited after review, merges, or missing prior captures may not match automatically. Attach a saved review explicitly with `/link REV` (the latest review) or `/link REV REVIEW-ID`.

Explicit associations are labelled as explicitly linked. Multiple reviews can be attached to a commit; linking the same review again is safe. References pin the captured session snapshots, so later reviews, changed transcripts, or deleted original transcript files do not replace the saved excerpts. Different captures of the same session remain distinguishable; identical captures are listed once with their review IDs. Missing saved evidence produces an error.

These links and excerpts stay in the local `.wy/wy.sqlite3` database. They are discovered during lookup, without a Git hook or background process. A clone on another machine does not receive them. A rewritten commit has a new hash and may need an explicit association. A review must contain saved conversation history for `/link` to attach it.

## Summaries and original turns

The importer classifies captured messages as original turns, compacted summaries, unknown provenance, or tool records. Compacted summaries display **Secondary evidence · Compacted summary** in the reader, sources, and commit conversations. A compaction marker without readable summary text still displays the missing context explicitly.

In a summary's source view or commit conversation, click **Open original turn**, or press **s**, select the source, and press Enter. Escape returns to the summary. The source opens the redacted saved message, pinned by snapshot, event identity and content hash, even if the raw transcript has been deleted. Retained original context has a distinct link label: it provides context, without claiming that every summary assertion is supported by that message.

Resolution uses explicit message/turn references and saved snapshots, with a fallback to a repository-matched local transcript. It never matches originals by similar wording or guesses an earlier user request. Resuming the same session can resolve references against earlier captures. Cross-session references currently require a directly recorded parent session and an inclusive turn cutoff; messages after that cutoff are excluded. Missing boundaries, indirect or unknown ancestry, conflicting captures, and missing originals remain unavailable. This is deliberately conservative when a provider omits provenance metadata.

**Original turn unavailable** appears explicitly when no original can be verified. Partial resolution says that some originals are unavailable. A summary or unclassified capture cannot supply a Recorded quote, and a judgment relying only on those sources must leave original rationale unknown. Code-based assessments remain possible when independently supported. An original assistant quote records what the assistant said; it does not authenticate authorship or prove the reason correct.

Older snapshots without provenance remain readable as **Unknown provenance**. Refresh with **r** to classify an available transcript in a new capture; pinned historical captures are preserved. Old saved answers lose unsupported Recorded/Inferred labels and display a warning about their earlier prose. Each lookup is bounded to 32 references, 20 saved captures, and 40 MB of captured data; exceeding a limit is reported as unavailable context. Provider format coverage is described in [Codex ingestion support](codex-formats.md).

## Agent explanations

Install and sign in to Codex or Claude Code, then ask from the workspace:

| Input | What it explains |
| --- | --- |
| **e** | The selected file, function or captured edit |
| `/why FILE:SYMBOL QUESTION` or `/why FILE:LINE QUESTION` | One function or line |
| `/ask QUESTION` | A follow-up about the displayed notes or answer |
| `/reason QUESTION` | All current changes |
| `/agent codex\|claude` | Chooses the answering agent |

Symbol targets use tree-sitter outlines; line targets work for any eligible source file. The agent receives bounded, redacted code, diff and observable history from a temporary directory with repository tools and configuration disabled. Output schemas, citations and source freshness are validated before saving. These are fresh assessments, not recovered private reasoning. Press **s** or a number to open the source behind a citation.

**Recorded** means an explicit relevant assistant justification was found. **Inferred** means an evidence-based hypothesis. **Unexplained** means motivation was not established. A recorded statement can still be wrong.
