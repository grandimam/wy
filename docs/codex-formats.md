# Codex ingestion support

Verified on 2026-10-08 using official documentation and a local rollout's **field names only**. The fixtures are synthetic; no private session history is checked into this repository.

The documented [`codex exec --json` event stream](https://learn.chatgpt.com/docs/non-interactive-mode) exposes lifecycle and item events. wy consumes completed agent messages, command executions, file changes and MCP tool calls. Commands and their aggregated output are stored as observable text, never run. The stream may omit the originating user prompt and workspace metadata. Reasoning items and usage data from the original coding run are intentionally not ingested as justifications or charged as wy analysis tokens.

Recent-code extraction also recognizes static patch literals passed to `tools.apply_patch` inside an `exec` wrapper. JavaScript is parsed, never evaluated; dynamic expressions and interpolated templates are skipped. Captures preserve code before the normal conversation-excerpt limit. A recorded call without an explicit success result remains labeled as unconfirmed. Turn and item IDs do not replace the identity from `session_meta`.

A best-effort local rollout adapter recognizes `session_meta`, `response_item` and `event_msg` JSONL records. It extracts user/assistant message text, function/custom tool calls and outputs, patch file paths, and observable user/agent message events. It deduplicates repeated normalized messages. Tool calls are classified into read, search, test, change or general tool categories with conservative lexical hints; classification does not prove the tool executed successfully. Call IDs preserve input/output correlation when present.

Private `reasoning` records, encrypted content, assistant analysis-channel messages, system/developer prompts, images and unknown event types are skipped. The local rollout format is internal and version-sensitive; its support is explicitly best-effort rather than a stability promise. Malformed JSON lines produce warnings. New unknown event types are ignored rather than interpreted as assistant statements.

Official [Codex app-server documentation](https://learn.chatgpt.com/docs/app-server) describes another observable integration interface. A live app-server subscriber is not implemented in this MVP. The history adapter boundary and stable JSON `Session`/`Event` schemas allow additional adapters without changing the decision engine.

Synthetic transcripts in `tests/session_code.rs` cover static wrapped patches, event versus session identity, repeated edits, known failures, repository scoping, redaction, recent-session limits, and Claude writes and edits. No private session history is checked into the repository.
