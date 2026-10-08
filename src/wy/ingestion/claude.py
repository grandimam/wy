"""Claude Code JSONL adapter: observable messages and tools, never thinking blocks."""

from __future__ import annotations

import json
import os
from pathlib import Path

from wy.ingestion.codex import MAX_SESSION_BYTES, _kind
from wy.models import Event, Session
from wy.security import redact


def visible_text(value) -> str:
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        return "\n".join(str(b.get("text", "")) for b in value
                         if isinstance(b, dict) and b.get("type") == "text")
    return ""


def metadata(path: Path) -> dict | None:
    # Claude may put snapshots/progress before its first message. Read a bounded
    # prefix and retain only workspace/session fields, never message content.
    try:
        with path.open(encoding="utf-8") as stream:
            remaining = 1_000_000
            for _ in range(100):
                line = stream.readline(min(remaining + 1, 256_000))
                remaining -= len(line)
                if not line or remaining < 0:
                    break
                try:
                    row = json.loads(line)
                except ValueError:
                    continue
                if isinstance(row, dict) and isinstance(row.get("cwd"), str) and row.get("sessionId"):
                    return {"id": str(row["sessionId"]), "cwd": row["cwd"], "path": str(path.resolve()),
                            "timestamp": row.get("timestamp"), "agent": "claude"}
    except (OSError, UnicodeError):
        pass
    return None


class ClaudeCollector:
    def discover(self, home: Path | None = None) -> list[dict]:
        home = home or Path(os.environ.get("CLAUDE_CONFIG_DIR", Path.home() / ".claude"))
        # Main project sessions only; subagent files are not silently merged with
        # their parent. They can be selected explicitly if they have valid cwd.
        return [item for path in sorted((home / "projects").glob("*/*.jsonl"))
                if (item := metadata(path))]

    def collect(self, path: Path) -> Session:
        if path.stat().st_size > MAX_SESSION_BYTES:
            raise ValueError("Session exceeds the 20 MB input limit")
        session = Session(id=path.stem, path=str(path.resolve()), agent="claude", format="claude-code-jsonl")
        seen = set()
        with path.open(encoding="utf-8") as stream:
            for number, line in enumerate(stream, 1):
                try:
                    row = json.loads(line)
                    if not isinstance(row, dict):
                        raise ValueError()
                except ValueError:
                    session.warnings.append(f"Skipped malformed session line {number}")
                    continue
                if isinstance(row.get("cwd"), str):
                    if session.cwd and session.cwd != row["cwd"]:
                        raise ValueError("Session contains conflicting working directories")
                    session.cwd = row["cwd"]
                if row.get("sessionId"):
                    if session.events and session.id != str(row["sessionId"]):
                        raise ValueError("Session contains conflicting session identities")
                    session.id = str(row["sessionId"])
                if row.get("type") not in {"user", "assistant"}:
                    continue
                message = row.get("message", {})
                if not isinstance(message, dict):
                    continue
                content = message.get("content", [])
                blocks = [{"type": "text", "text": content}] if isinstance(content, str) else content
                if not isinstance(blocks, list):
                    continue
                for block_index, block in enumerate(blocks):
                    if not isinstance(block, dict):
                        continue
                    kind, tool, call_id, files = None, None, None, []
                    text = ""
                    if block.get("type") == "text":
                        kind, text = row["type"], str(block.get("text", ""))
                    elif block.get("type") == "tool_use" and row["type"] == "assistant":
                        tool = str(block.get("name", ""))
                        args = block.get("input", {})
                        text = json.dumps(args, ensure_ascii=False)
                        call_id = block.get("id")
                        kind = {"Read": "read", "Glob": "search", "Grep": "search",
                                "Edit": "change", "MultiEdit": "change", "Write": "change"}.get(tool, _kind(tool, text))
                        if isinstance(args, dict) and isinstance(args.get("file_path"), str):
                            files = [args["file_path"]]
                    elif block.get("type") == "tool_result" and row["type"] == "user":
                        kind = "tool_output"
                        call_id = block.get("tool_use_id")
                        text = visible_text(block.get("content", ""))
                    # thinking, redacted_thinking, images and unknown blocks are omitted.
                    if not kind or not text:
                        continue
                    event_id = f"event-{number}-{block_index}"
                    identity = (row.get("uuid", str(number)), block_index)
                    if identity in seen:
                        continue
                    seen.add(identity)
                    session.events.append(Event(id=event_id, kind=kind, text=redact(text)[:16000],
                                                source_line=number, tool=tool, call_id=call_id, files=files))
        return session
