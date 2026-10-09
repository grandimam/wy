# Codex ingestion support

Verified on 2026-10-09 using official documentation and local rollouts' **field names only**. The fixtures are synthetic; no private session history is checked into this repository.

The documented [`codex exec --json` event stream](https://learn.chatgpt.com/docs/non-interactive-mode) exposes lifecycle and item events. wy consumes completed agent messages, command executions, file changes and MCP tool calls. Commands and their aggregated output are stored as observable text, never run. The stream may omit the originating user prompt and workspace metadata. Reasoning items and usage data from the original coding run are intentionally not ingested as justifications or charged as wy analysis tokens.

Recent-code extraction also recognizes static patch literals passed to `tools.apply_patch` inside an `exec` wrapper. JavaScript is parsed, never evaluated; dynamic expressions and interpolated templates are skipped. Captures preserve code before the normal conversation-excerpt limit. A recorded call without an explicit success result remains labeled as unconfirmed. Turn and item IDs do not replace the identity from `session_meta`.

A best-effort local rollout adapter recognizes `session_meta`, `response_item` and `event_msg` JSONL records. It extracts user/assistant message text, function/custom tool calls and outputs, patch file paths, and observable user/agent message events. Deduplication preserves distinct message and turn identities when present. Tool calls are classified into read, search, test, change or general tool categories with conservative lexical hints; classification does not prove the tool executed successfully. Call IDs preserve input/output correlation when present.

Private `reasoning` records, encrypted content, assistant analysis-channel messages, system/developer prompts, images and unknown event types are skipped. The local rollout format is internal and version-sensitive; its support is explicitly best-effort rather than a stability promise. Malformed JSON lines produce warnings. New unknown event types are ignored rather than interpreted as assistant statements.

Official [Codex app-server documentation](https://learn.chatgpt.com/docs/app-server) describes another observable integration interface. A live app-server subscriber is not implemented in this MVP. The history adapter boundary and stable JSON `Session`/`Event` schemas allow additional adapters without changing the decision engine.

## Compaction provenance

The adapter marks `compacted` records, summary-phase messages, and completed `contextCompaction`/`context_compaction` markers as secondary evidence. It preserves a readable `payload.message`, or a recognizable public summary wrapper in `replacement_history`, without treating replacement history as original speech. Opaque markers produce a visible placeholder. Common continuation-summary wrappers without an explicit provider marker are classified as unknown. This wrapper check is a conservative heuristic, not a general summary detector.

When a rollout exposes `retained_context`, complete public user/assistant messages with `message_id` and `turn_id` can be captured separately as originals. Incomplete messages, analysis content, and summary-phase retained items are excluded. Links from the summary to these messages say **retained original context**; the relationship does not establish support for each summary claim. Turn IDs come from `turn_context`/`task_started` or message metadata; message IDs are preserved when present.

An optional import extension, `source_refs: [{session_id, turn_id, message_id}]`, accepts explicit original references on a row or payload. At least a message ID or turn ID is required; omitted session IDs refer to the current session. This extension is **not** a claim that all Codex or Claude versions emit such references. Without references or explicitly retained originals, wy displays **Original turn unavailable**.

The app-server documentation describes `forkedFromId` and an inclusive `lastTurnId` fork cutoff; these fields are not guaranteed in local rollout files. The adapter preserves optional `forked_from_id`/`forkedFromId`, `resumed_from_id`, and `forked_from_turn_id`/`lastTurnId` session metadata. Cross-session resolution requires a direct parent and a verified cutoff in that parent's captured events. Unrecorded boundaries and indirect ancestry remain unavailable rather than being reconstructed from timestamps or prose.

The Claude adapter preserves `uuid` and `parentUuid`, classifies `isCompactSummary` messages and compaction content blocks as summaries, and represents `compact_boundary` system markers with a fixed public placeholder. It does not import the system record's text or infer cross-session ancestry from `parentUuid` alone.

`tests/provenance.rs` covers summaries, opaque markers, retained originals, resumed captures with reused line numbers, deleted transcripts, bounded forks, conflicts, repository scoping, and rejection of summary-based Recorded/Inferred rationale. TUI tests cover opening pinned originals by keyboard and mouse and returning to the summary. These adapters remain best-effort as transcript formats evolve.

Synthetic transcripts in `tests/session_code.rs` cover static wrapped patches, event versus session identity, repeated edits, known failures, repository scoping, redaction, recent-session limits, and Claude writes and edits. No private session history is checked into the repository.
