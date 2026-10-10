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
> WY_DECISION
> - File / symbol: src/session_repository.rs :: SessionRepository
> - Decision: Separate persistence from session discovery.
> - Requirement: Support JSONL and SQLite sources without duplicating persistence.
> - Reason: A shared storage boundary lets each adapter focus on its input format.
> - Alternatives / tradeoffs: Inline persistence is simpler initially; the shared
>   boundary adds an abstraction but avoids adapter-specific cache behavior.
> - Evidence: Relevant user request, inspected files, and tool-call IDs if available.
> - Related edits: Files/symbols changed and edit IDs if available.
> - Validation: Tests actually run and their results, or explicitly "not yet run".
> - Timing: Decision-time record, revision, or retrospective explanation.

These records are retained verbatim as assistant statements, not parsed into
self-certified causal claims. File references and neighboring edits make them
available to wy's existing recorded-notes and explanation evidence selection.
Proximity alone is not proof that a statement caused an edit. For existing sessions,
wy can only use the rationale and evidence that were actually saved.

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
