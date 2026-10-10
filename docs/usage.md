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
History section. Other sessions appear only when you open the session picker with
**b**, `/sessions`, or **Change session**.

At startup wy selects the latest captured session using known source-event dates.
It does not claim that session is currently active: wy observes agent history,
not the interface that launched the agent. Unknown dates remain unknown; file
modification times are never used to invent recency. The picker identifies the
latest dated capture and the selected session.

Refreshing with **r** captures history again offline and retains the selected
session by tool and session ID when available. If the source filter or capture
budget excludes it, wy selects another available session. No model runs on startup,
refresh, section changes, or session selection.

## Read decisions

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
bounded captured edits and public context from **only the selected session** to
your configured Codex or Claude CLI. It returns at most eight choices ordered by
assessed consequence. It may consume your signed-in account's usage.

Every affected file must cite captured-edit evidence from that session. Recorded
rationale additionally requires an exact cited original assistant statement.
Alternatives and trade-offs are new assessments unless explicitly attributed to
a recorded statement. Structural validation does not prove semantic accuracy.

If there are no captured code edits, decision discovery does not substitute today's
files. Explicit recorded decisions can still be read, with missing code links
identified. Unsupported edits, omitted context and truncation remain gaps.

Briefs are saved against the repository and immutable session snapshot. Later
commits, current-code edits, or file deletion do not invalidate a historical brief.
New captured content for the session does. A background result never changes the
selected session, interrupts a draft, or replaces a deep dive. **x** cancels requests.

## Session implementation flow

Sessions gives each original request a bordered card. **Agent response** and
**Agent notes** sit directly beneath it, collapsed by default, followed by the
captured changes. Planning turns without edits remain available. Notes are
captured context, not verified reasons; unavailable notes are labeled explicitly.

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

Pages contain at most 20 edits and eight request groups. Long requests continue
across pages without dropping edits. Previous/Next controls or Alt+Left/Right move
between pages. Returning from Decisions preserves the selected snapshot's flow
position. **Original context** opens the saved conversation, with optional tool
activity and metadata. Session transcripts paginate at 20 events per page.

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
