<div align="center">

<img src="docs/assets/wy-owl-upright.png" alt="wy's front-facing pixel owl mascot" width="144" height="144">

# wy

### Understand agent-generated changes—with the evidence beside the code.

[![Experimental](https://img.shields.io/badge/Status-Experimental-E7B66D?style=flat-square&labelColor=182335)](#status)
[![History support](https://img.shields.io/badge/History-Codex_%2B_Claude_%2B_pi_%2B_OpenCode-78DCCE?style=flat-square&labelColor=182335)](#how-it-works)
[![Offline by default](https://img.shields.io/badge/Offline-by_default-78DCCE?style=flat-square&labelColor=182335)](#what-the-evidence-means)
[![MIT License](https://img.shields.io/badge/License-MIT-A9A1FF?style=flat-square&labelColor=182335)](LICENSE)

</div>

Your agent changed 40 files. The diff tells you what changed; the context is buried
in conversations, tool results, and earlier attempts. wy brings those together so
you can understand the implementation and decide what still needs checking.

It imports existing local **Codex, Claude Code, pi, and OpenCode** history. No
recorder has to be running first. Missing evidence stays missing—wy does not turn
a nearby message into a proven reason for a change.

![wy terminal UI: files beside captured conversation and diff hunks](docs/assets/terminal-preview.png)

*The screenshot illustrates the reader layout; labels may differ in current builds.*

## Get started

Needs Rust 1.88+, Git and a C toolchain.

```bash
cargo install --git https://github.com/grandimam/wy --locked wy-code
cd /path/to/your/repo
wy
```

- **Pick a file:** inspect its diff alongside matching edit records and nearby conversation.
- **Enter on a linked change:** open the captured turn, with tool, session and dates.
- **Explanation tab:** click **Ask AI to explain this change** for a new, cited assessment through your signed-in Codex or Claude CLI. `e` is the shortcut; opening the tab alone never calls a model.
- **r:** capture current changes and history again.

**Changes / Explanation / History** tabs keep code, new assessments, and historical context separate (`o` / `v` / `t`). The sidebar separates current changes from earlier sessions. Your saved request leads each group of changes in a padded Ratatui card, expanded but collapsible, above a separate code panel. Historical code, agent responses and notes stay collapsed until opened. **View original chat** opens the saved conversation; raw patches and session metadata are tucked into **Technical details**. Short confirmations show earlier request context when available; missing requests remain unknown. `z` folds/unfolds code. History is paginated at 20 events per page, with Previous/Next buttons.

**Tab** switches panes · **w** collapses nearby context · **?** opens help.

## Understand history and decisions

| Command | What it shows |
|---|---|
| `/coverage` | Per-tool discovery/capture counts, exclusions, warnings, and unmatched hunks |
| `/sessions` | Separate sessions across tools, start/last-event dates, and models; Enter opens one |
| `/timeline [FILE:SYMBOL]` | Chronological turns associated with a file or explicitly mentioned symbol |
| `/decisions [FILE:SYMBOL]` | Structured decisions, requirements, available rationale, captured tests, and review gaps |
| `/source all` | Include all four tools; `both` remains a compatibility alias |
| `/source pi` | Select one history tool; press **r** to refresh |
| `/setup` then `/setup save` | Preview and save optional decision-record instructions; does not modify agent configuration |
| `/export` then `/export save` | Preview and save the exact offline review brief locally; nothing uploaded |

Omitting `FILE:SYMBOL` uses the selected file/symbol. Symbol filtering is based on
explicit text/record matches, not semantic causality. A timeline preserves tool and
session boundaries; later edits are flagged as potentially making earlier context
inapplicable, not automatically declared to supersede it.

Dates distinguish **when the agent recorded an event** from **when wy captured
history**. Absolute UTC dates and relative ages are shown; missing dates are marked
unknown rather than inferred from transcript file modification times.

## How it works

1. Discover supported local histories whose recorded working directory belongs to this Git repository.
2. Capture up to 20 sessions / 40 MB, alternating between tools; `/coverage` explains exclusions.
3. Compare changed-line text against recorded edits. An overlap establishes a recorded edit candidate, not authorship or intent.
4. Surface original requests/statements, available rationale, and summaries with distinct labels.
5. Optionally capture structured `WY_DECISION` statements during future coding, or request a **new retrospective assessment** through a reasoning CLI.

Recent-code browsing keeps the latest recorded edit per file from up to three
recent sessions (seven-day window for known timestamps, at most 50 files). The
session/timeline views inspect the broader **captured** history, not every session
that may exist on disk. Tool invocation still supports Codex and Claude only.

## What the evidence means

- **Recorded decision:** a self-reported structured explanation, not independently verified truth.
- **Nearby conversation:** captured context; proximity does not prove causation.
- **Available rationale:** readable provider-exposed thinking or reasoning summaries; tentative, not a verified justification.
- **Compaction summary:** secondary context, not original speech.
- **Inferred explanation:** a new model assessment, separate from historical intent.
- **Unknown:** evidence was not captured or cannot establish the claim.

Encrypted/redacted hidden reasoning cannot be recovered. Shell commands, formatters,
and manual changes often have no supported edit records. A missing record does
not mean the agent had no reason or that tests were never run.

Offline views never call a model. Requesting an explanation sends selected evidence
to your configured reasoning CLI. Exports exclude raw tool outputs and tentative
rationale, apply best-effort redaction, and require a preview before saving—**inspect
for sensitive content before sharing**.

## Status

**Experimental (0.2.0).** Useful for reviewing substantial local agent changes and
returning to unfamiliar code. Not a replacement for code review or a guarantee of
original intent. Older OpenCode JSON storage is detected but not imported. Some
provider formats and shell-driven edits remain unsupported.

Automated tests establish implementation behavior, not explanation accuracy or
user value. See the [evaluation protocol](docs/evaluation.md) for measuring those.

## Learn more

[Usage](docs/usage.md) · [Decision records](docs/decision-records.md) · [Architecture](docs/architecture.md) · [Security](docs/security.md) · [Codex formats](docs/codex-formats.md) · [Releasing](docs/releasing.md)

Development: `cargo test --locked`. Licensed under [MIT](LICENSE).
