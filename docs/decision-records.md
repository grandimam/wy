# Capture why an agent changed code

wy reads user requirements, assistant statements, recorded edits, tool results,
validation runs, and compaction summaries from Codex, Claude Code, pi, and OpenCode.
Use `/source all` (the default `both` is a compatibility alias), or select an agent.
Refresh with **r** after coding to capture new history.

Readable Claude/pi thinking blocks, OpenCode reasoning parts, and Codex reasoning
summaries are captured as **rationale**. They are tentative context, not verified
reasons for a particular edit. They may describe alternatives that were abandoned.
Encrypted signatures, encrypted reasoning, images, and redacted blocks are not
imported. Codex analysis-channel messages remain excluded. Capturing a transcript
cannot recover hidden information that a provider did not expose.

Compaction summaries are secondary context, never proof of an original decision.
An explanation generated later is an inference unless supported by a recorded
statement and evidence. The absence of rationale means **unknown**, not that the
agent had no reason. Session captures are local, but selected evidence is sent to
the configured reasoning CLI when you explicitly request an explanation. Readable
rationale receives the same redaction and size limits as other history.

## Decision-record instruction for coding agents

Copy the following instruction into your coding agent's project instructions (wy
never edits those instructions automatically):

> For significant decisions—especially new classes, abstraction boundaries,
> dependencies, and rejected alternatives—emit a concise public decision record
> in your assistant response when making the decision. This is a high-level
> explanation, not a request for private chain-of-thought. Include repository-relative
> file paths and symbol names so history can link it to the affected code. Describe
> evidence you actually observed; do not invent references or retroactive reasons.
> Update the record if tests or requirements change the decision.
>
Use **JSON**, not a bullet list, after each `WY_DECISION` marker:

```text
WY_DECISION
{
  "file": "src/session_repository.rs",
  "symbol": "SessionRepository",
  "decision": "Separate persistence from discovery",
  "reason": "Both JSONL and SQLite adapters need the same saved-history boundary",
  "requirement": "Support multiple coding tools",
  "alternatives": ["Inline persistence inside each adapter"],
  "tradeoffs": ["Adds an abstraction; avoids duplicated persistence behavior"],
  "evidence": ["Actual inspected file or message/tool event reference"],
  "related_edits": [],
  "validation": "Not yet run",
  "timing": "decision-time"
}
```

This example demonstrates the format, not a decision to apply. Multiple markers
in one assistant response are supported. For a choice spanning files, repeat the
same choice, reason, alternatives and trade-offs with each relevant file path;
the decision overview groups identical assertions across those files. Different
reasons remain separate rather than silently resolving conflicting records.

`/setup` previews the full template; `/setup save` saves it locally. Records are
parsed only from original assistant messages and retained as self-reported
statements, not proven causal claims. A record and edit mentioning the same file
within a session do not establish semantic causality or current applicability.
User messages and compaction summaries cannot declare recorded decisions.

## Selected-session decision overview

wy opens on decisions in the **latest dated captured session**, without asserting
that the session is currently active. `d` returns to decisions; `t` opens the
selected session's implementation flow; `b` or `/sessions` chooses another session.
Offline records are ordered by affected-file count, not assessed importance.
Records and code links remain scoped to that one captured session.

Choose **Identify decisions with AI**, press `e`, or use `/decisions discover` for
a bounded retrospective assessment. This explicit action sends only the selected
session's captured edits and context to the chosen Codex or Claude CLI. It groups
choices across files and returns at most eight decisions ordered by assessed
consequence. No captured edits means discovery is unavailable: current source code
is never substituted for missing historical code.

Each detail shows why, significance, alternatives, trade-offs, captured code and
original context. Recorded/inferred/unknown rationale stays visible. Alternatives
are assessments unless explicitly attributed to a cited record. Every affected
file must cite a captured edit from the selected session. Recorded rationale
requires an exact original assistant quote; validation checks references, not
semantic truth. An input with unconfirmed execution is not a proven implementation.

Briefs are cached by repository and pinned session snapshot, independently of Git
HEAD and current files. Committing or deleting files does not erase captured work;
new captured session content invalidates the cached brief. Current-code comparison
is an explicit drill-down, never part of the session inference packet. Partial
patches cannot establish full historical files and are never replayed onto today's
code. `/changes` keeps working-tree changes separate, with attribution unknown.

## Storage adapters

- pi: `$PI_CODING_AGENT_DIR/sessions`, default `~/.pi/agent/sessions`, plus
  repository-local `.pi/sessions`. Capture follows the last recorded branch.
- OpenCode: `$XDG_DATA_HOME/opencode/opencode.db` (default
  `~/.local/share/opencode/opencode.db`), plus `opencode-dev.db` if present.
  Databases are opened read-only. Session directories must belong to the current
  Git repository. Source positions in SQLite captures are normalized event
  positions, not physical database lines. Older JSON-file OpenCode stores are not
  supported yet.

Reasoning CLI execution still supports Codex and Claude only; history support for
pi/OpenCode does not enable running them as explanation providers.
