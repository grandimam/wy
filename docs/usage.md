# Using wy

Install from a checkout:

```bash
cargo install --path . --locked --force
```

Or from GitHub:

```bash
cargo install --git https://github.com/grandimam/wy --locked wy-code
```

Requires Rust 1.88+, Git and a C toolchain. Run `wy` inside your repository, or
`wy --repo /path/to/repository`. `cargo run --` runs the checkout without installing.
The workspace requires an interactive terminal; there are no subcommands.

## Decisions and Sessions

The left navigation has two sections:

- **Decisions (`d`)**: consequential choices from the selected captured session.
- **Sessions (`t`)**: that session's requests and chronological recorded edits.

On narrow terminals, these become a compact top row. Ctrl+Left/Right switches
sections. Original conversations are available within a session—not in a separate
History section. Captured sessions appear beneath **Sessions** in the left navigation.
The session picker remains available through **b** or `/sessions`.

At startup wy selects the latest captured session using known source-event dates.
It does not claim that session is currently active: wy observes agent history,
not the interface that launched the agent. Unknown dates remain unknown; file
modification times are never used to invent recency. The picker identifies the
latest dated capture and the selected session.

Refreshing with **r** captures history again offline and retains the selected
session by tool and session ID when available. If the source filter or capture
budget excludes it, wy selects another available session. No model runs on startup,
refresh, section changes, or session selection.

## Copy text

Drag over text in the main pane to highlight it. Releasing the mouse copies the
selection to the clipboard; no Ctrl+Y or Cmd+C is needed. Selection stays inside
the main pane, excluding adjacent navigation. Use the wheel while holding the
mouse button, or hold the pointer above/below the content to scroll and extend
the selection. Click a link without dragging to open it.

On macOS, copying uses `pbcopy`. Linux uses `wl-copy`, `xclip`, or `xsel` when
available, otherwise wy sends an OSC 52 clipboard request (terminal support and
permission required, including over SSH). Plain **Ctrl+C** still quits wy.

**Ctrl+Y** remains an optional terminal-native copy mode: it isolates the main
content and releases mouse capture. Use the terminal's copy shortcut in that
mode, then **Esc** to return to the normal layout.

## Read decisions

### Choose the work to explain

In a request's Detail view, select **Explain this request’s decisions**. This opens
its decision overview without calling a model. The header shows the selected scope,
configured provider, and whether a generated brief exists. **Generate decision brief**
explicitly sends the selected captured evidence; failures offer retry, and **x** cancels.
An ungenerated brief is distinguished from an assessment that found no consequential choices.

- `/decisions request`: open decisions for the request currently being read.
- `/decisions requests 2-4`: include adjacent requests 2 through 4.
- `/decisions requests 2`: select request 2 only.
- `/decisions session`: return to the entire-session scope.

Numbers are the one-based request positions displayed in Detail, not provider request IDs.
Request boundaries are not automatically detected task boundaries: include adjacent requests
explicitly when follow-ups belong to the same work. Earlier-request context is not silently
sent with a selected request. Pressing **e** in request Detail generates for that request;
in Decisions it generates for the displayed scope.

Scoped records, edits, and rationale come only from the selected requests. The same file
appearing elsewhere in the session does not make that other request's evidence eligible.
Brief caches include the immutable snapshot, normalized selection, and analysis version.
If refresh changes the snapshot, the request selection is cleared rather than reusing
ordinals against different content. Changing history source requires confirming the refreshed
scope before generation. Existing session-wide briefs remain stored, but this analysis-version
change requires regeneration before reuse.

### Inspect the brief

The overview groups identical explicit `WY_DECISION` assertions across files in
one session. Different reasons remain separate. Original assistant statements are
self-reported evidence, not independent verification. File association does not
prove that a statement caused a particular edit.

Select a decision with Up/Down and Enter, or click it. Read its rationale,
alternatives and trade-offs, then open code evidence. Original context is the last
drill-down. **Recorded / Inferred / Unknown** rationale stays visible; general
caveats and snapshot metadata are collapsed under **Details**.

Press **e** on Decisions or the session flow, click **Identify decisions with AI**,
or use `/decisions discover` to request a retrospective assessment. This sends
bounded captured edits and public context from **only the selected work within the session** to
your configured Codex or Claude CLI. It returns at most eight choices ordered by
assessed consequence. It may consume your signed-in account's usage.

Every affected file must cite captured-edit evidence from that session. Recorded
rationale additionally requires an exact cited original assistant statement.
Alternatives and trade-offs are new assessments unless explicitly attributed to
a recorded statement. Structural validation does not prove semantic accuracy.

If there are no captured code edits, decision discovery does not substitute today's
files. Explicit recorded decisions can still be read, with missing code links
identified. Unsupported edits, omitted context and truncation remain gaps.

Briefs are saved against the repository, immutable session snapshot, and selected request scope. Later
commits, current-code edits, or file deletion do not invalidate a historical brief.
New captured content for the session does. A background result never changes the
selected session, interrupts a draft, or replaces a deep dive. **x** cancels requests.

## Session implementation flow

The workspace uses **Sessions | Detail | Requests**. The left navigation lists
session IDs from a metadata catalog, newest known source date first.
Sessions has 10 IDs per page, a session count, and Previous/Next controls.
PageUp/PageDown or Alt+Left/Right change pages while Sessions is focused; paging
does not import a transcript until you select it. The TUI catalog includes all
discovered supported sessions and saved snapshots, without the old 20-session or
40 MB aggregate capture cap. Only the selected transcript is retained in memory.
Dates for unimported sessions come from source metadata, not file modification times.
The importer still has a 20 MB per-transcript guard; `/coverage` distinguishes
this from the separate bounded AI-evidence capture path.
Select a session on the left and a request on the right; **Detail** shows only that
request's conversation and changes. Tab cycles Detail → Sessions → Requests;
Shift+Tab reverses direction. Use Up/Down and Enter to select. On narrow terminals,
focus a side pane with Tab to reveal it. On wide terminals, drag the dividers beside
Sessions and Requests to resize them. `[` / `]` shrink/grow the focused side pane.
Widths are saved per repository; `/layout reset` restores defaults. Detail retains
at least 40 columns when all three panes are visible.

The top-right **Explain with: codex** (or another agent) identifies the configured
explanation agent, not the source of the selected historical session.

Detail shows **YOU** and **AGENT** in separate, always-visible rounded cards,
with role-colored titles, padding, and text wrapping to the pane width. Borders
remain aligned while scrolling or resizing; neither message requires expansion.
Fenced code blocks (backticks or tildes) inside either message show a language label
and snippet-relative line numbers. Code preserves indentation and blank lines, stays
unwrapped, and pans with Left/Right or h/l while the reader is focused; gutters stay
fixed and surrounding prose still wraps. Tabs display as four spaces. Unknown languages
use plain code styling; syntax highlighting is not yet applied. Unclosed fences run only
to the end of that message and are labeled explicitly. These are conversation snippets,
not verified file edits or executable actions.
The header combines the
request ID, agent, and session date. Changes, agent notes, and tool activity remain
expandable; empty sections are omitted. Session-level capture warnings are available
under **⚠ Capture warning**, collapsed by default. Edit execution uncertainty stays
beside the changes it qualifies. Planning turns without edits remain available.
Notes are captured context, not verified reasons.
Conversation text is no longer shortened during capture or in Detail: responses
are shown fully, and expanding notes reveals their full captured text, with secret
redaction retained.
Older snapshots may already contain shortened text; press `r` to recapture from
available originals. Source omissions and compaction cannot be recovered by expanding
a section. Code edits, Git diffs, and comparison text no longer have 40,000-character
excerpt limits; the 64-edit parser cap and 32-message retained-context cap are removed.
Source-file admission limits, import guards, and AI evidence budgets still apply.

Change lists longer than three edits start collapsed. Execution uncertainty and
partial-capture notices appear once per affected group, outside the disclosure.
Multiple edits to the same file remain separate and in recorded order—not a
reconstructed runtime call graph. Short confirmations such as “yes do it” stay
literal; **Earlier request** optionally reveals separately labeled context.

Open an edit to see **As implemented · captured edit**. Code comes from captured
patches or writes, never from the current file presented as historical code.
Known failed edits are excluded; recorded inputs whose execution cannot be
confirmed are labeled accordingly. Patches are replacement excerpts, not complete
historical files. Truncated and empty excerpts are marked explicitly.

The **Requests** pane lists captured request IDs, 20 per page, rather than message
previews. Missing IDs are explicitly labeled unavailable; Detail retains the message text. Previous/Next controls,
Alt+Left/Right, or PageUp/PageDown while Requests is focused change its page and
select the first request on that page. Short terminals scroll the visible page's
list independently. Long requests and all their edits stay together in Detail,
which scrolls as needed. Switching sessions restores each session's selected
request and page; returning from Decisions also preserves the detail position. Captured **Tool activity** is expandable beneath the corresponding
request, alongside responses and notes; there is no separate conversation footer.

Request pages are backed by SQLite: each page reads 20 IDs and only the selected
request's detail, including complete captured edits. Indexing an older snapshot
happens once on selection. Only two full reader documents are cached; selection
positions are retained separately. This is not yet a fully streaming importer or
virtualized detail renderer: the selected transcript is still parsed in memory,
and expanding a very large request can allocate its full rendered content.

Captured code remains readable after it is committed, changed, deleted, or the
original transcript is removed. This requires a saved, verifiably repository-scoped
snapshot; wy cannot recover edits that were never captured.

## Compare with the present

Inside an edit, **Compare with current code** shows two explicitly separate views:

1. **As implemented**: the captured historical edit.
2. **Current code · read now**: the present file, read only on this action.

A complete, successful final captured write can show **Matches captured text** or
**Changed since captured edit**. Comparison uses redacted text and ignores final
newlines. It does not establish that a decision was reversed, superseded, or changed
by a particular author. A difference may have occurred later within the same session.

Partial patches, truncated writes, unconfirmed execution, and writes followed by
another captured edit cannot establish a full-file comparison. The two excerpts
remain inspectable, but wy reports comparison unavailable. Missing/unreadable code
is also reported as unavailable—not automatically declared deleted.

wy never applies historical patches to today's tree to invent an earlier base.
Session inference packets contain no present-day code, even after a comparison
has been opened.

## Working-tree changes are separate

`/changes` shows HEAD-to-working-tree changes, including staged, unstaged and
eligible untracked content, with **attribution unknown**. These changes are not
assigned to the newest or selected session. Multiple sessions, manual edits and
formatters may all have contributed. The view is bounded to 40 files and 400
rendered lines per file, with captured raw patches available in Technical details.

A clean Git tree does not mean the selected session has no work. Likewise, a dirty
tree does not establish that the selected session owns those changes.

## Controls

| Key | Action |
| --- | --- |
| `d` / `t` | Decisions / selected session work |
| Ctrl+Left / Ctrl+Right | Switch primary sections |
| `b` or `/sessions` | Choose another captured session |
| Up/Down or `j`/`k` | Select links or scroll |
| Enter | Select/open a link |
| `s` | Enter link selection |
| `1`–`9` | Open a decision or a numbered source |
| `e` / `R` | Explicitly discover/refresh session decisions |
| `z` | Fold/unfold code blocks |
| Left/Right or `h`/`l` | Pan code horizontally |
| PageUp/PageDown, Home/End | Navigate the reader |
| Alt+Left/Right | Previous/next page |
| Escape | Leave link selection or return to the previous view |
| `r` | Refresh offline capture |
| `x` | Cancel requests |
| `?` / F1 | Help |
| `q` / Ctrl+Q / Ctrl+C | Quit |

Mouse clicks open navigation sections, decisions, edits and disclosures. The wheel
scrolls the reader. Input supports paste and Ctrl+U to clear.

## Capture and additional tools

Use `/agent codex|claude` to choose the assessment CLI. `/source
all|codex|claude|pi|opencode|none` selects imported history (`both` aliases `all`).
The next refresh/request applies the filter.

- `/coverage`: discovery counts, exclusions, warnings and current-diff linkage gaps.
- `/timeline FILE[:SYMBOL]`: related captured turns across tools, keeping boundaries.
- `/decisions FILE[:SYMBOL]`: file-specific recorded context, tests and gaps across
  captured history; distinct from the default selected-session decision brief.
- `/setup`, then `/setup save`: preview/save optional public decision-record
  instructions. This does not change `AGENTS.md` or agent configuration.
- `/export`, then `/export save`: preview/save the **working-tree review**, not the
  selected-session AI brief. Raw transcripts/tool outputs and tentative rationale
  are excluded; redaction is best-effort. Nothing is uploaded. Inspect before sharing.
- `/why FILE:SYMBOL QUESTION` and `/reason QUESTION`: separate, explicit current-code
  explanation tools. Their packets may include current repository context; they are
  not the selected-session decision discovery path.
- `/cancel`: cancel pending requests.

History capture is bounded to 20 sessions and 40 MB total, with a 20 MB limit per
transcript. Captured artifacts are stored in `.wy/` inside the repo. They are local,
not encrypted; add `.wy/` to `.gitignore`. No capture can recover provider-hidden
reasoning, unsupported edits or absent transcripts. See [Decision records](decision-records.md)
and [Security](security.md).

## Find conversations by commit

**g**, `/commits`, or `/commit REV` opens saved commit conversations. Automatic
matching uses a saved review's base and source hashes, not an assumption of session
ownership. Partial commits, later edits, merges and missing captures may not match.
`/link REV [REVIEW-ID]` explicitly associates a saved review with a commit. Each
association retains its basis and pins captured session snapshots. A rewritten
commit may need a new explicit link. These tools do not change the selected session.

## Prebuilt installer

After the first binary release is published, the release workflow provides macOS
and Linux binaries:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/grandimam/wy/releases/latest/download/wy-code-installer.sh | sh
```

The installer selects your platform, verifies the checksum and installs into
`~/.local/bin`. Git is still required. See [Releasing](releasing.md) for availability.
