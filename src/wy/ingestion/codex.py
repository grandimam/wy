from __future__ import annotations

import json
import os
import re
from pathlib import Path

from wy.models import Event, Session
from wy.security import redact

MAX_SESSION_BYTES = 20_000_000


def _text(value) -> str:
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        return "\n".join(
            str(x.get("text", ""))
            for x in value
            if isinstance(x, dict) and x.get("type") in {"input_text", "output_text", "text"}
        )
    return json.dumps(value, ensure_ascii=False)


def _kind(name: str, text: str) -> str:
    if "apply_patch" in name:
        return "change"
    if re.search(r"\b(pytest|unittest|cargo test|npm test|npm run test|uv run.*pytest)\b", text):
        return "test"
    if re.search(r"\b(rg|grep|find)\b", text):
        return "search"
    if re.search(r"\b(cat|sed|head|read_file)\b", text):
        return "read"
    return "tool_call"


class CodexCollector:
    def collect(self, path: Path) -> Session:
        if path.stat().st_size > MAX_SESSION_BYTES:
            raise ValueError("Session exceeds the 20 MB input limit")
        session = Session(id=path.stem, path=str(path.resolve()))
        seen: set[tuple[str, str]] = set()
        with path.open(encoding="utf-8") as stream:
            for line_no, line in enumerate(stream, 1):
                try:
                    row = json.loads(line)
                    if not isinstance(row, dict):
                        raise ValueError()
                except ValueError:
                    session.warnings.append(f"Skipped malformed session line {line_no}")
                    continue
                typ = row.get("type")
                p = row.get("payload", {})
                if not isinstance(p, dict):
                    continue
                if typ == "session_meta":
                    session.id = str(p.get("id", p.get("session_id", session.id)))
                    session.cwd = p.get("cwd")
                    continue
                if typ == "thread.started":
                    session.id = str(row.get("thread_id", session.id))
                    session.format = "codex-exec-json"
                    continue
                kind, text, tool, call_id, files = None, "", None, None, []
                if typ == "response_item":
                    pt = p.get("type")
                    if (
                        pt == "message"
                        and p.get("role") in {"user", "assistant"}
                        and p.get("channel") != "analysis"
                        and p.get("phase") != "analysis"
                    ):
                        kind, text = p["role"], _text(p.get("content", []))
                    elif pt in {"function_call", "custom_tool_call"}:
                        tool = str(p.get("name", ""))
                        text = _text(p.get("arguments", p.get("input", "")))
                        kind = _kind(tool, text)
                        call_id = p.get("call_id")
                        files = re.findall(r"\*\*\* (?:Add|Update|Delete) File: (.+)", text)
                    elif pt in {"function_call_output", "custom_tool_call_output"}:
                        kind, text, call_id = "tool_output", _text(p.get("output", "")), p.get("call_id")
                elif typ == "event_msg" and p.get("type") in {"user_message", "agent_message"}:
                    kind = "user" if p["type"] == "user_message" else "assistant"
                    text = _text(p.get("message", ""))
                elif typ == "item.completed":
                    item = row.get("item", {})
                    if not isinstance(item, dict):
                        continue
                    it = item.get("type")
                    if it == "agent_message":
                        kind, text = "assistant", str(item.get("text", ""))
                    elif it == "command_execution":
                        tool, text = "shell", str(item.get("command", ""))
                        kind = _kind(tool, text)
                        text += "\n" + str(item.get("aggregated_output", ""))
                    elif it == "file_change":
                        kind = "change"
                        files = [
                            c["path"]
                            for c in item.get("changes", [])
                            if isinstance(c, dict) and isinstance(c.get("path"), str)
                        ]
                        text = json.dumps(item.get("changes", []))
                    elif it == "mcp_tool_call":
                        kind, tool = "tool_call", str(item.get("tool", ""))
                        text = _text(item.get("arguments", {})) + "\n" + _text(item.get("result", ""))
                if kind and text:
                    cleaned = redact(text)[:16000]
                    key = (kind, cleaned)
                    if key in seen:
                        continue
                    seen.add(key)
                    session.events.append(
                        Event(
                            id=f"event-{line_no}",
                            kind=kind,
                            text=cleaned,
                            source_line=line_no,
                            tool=tool,
                            call_id=call_id,
                            files=files,
                        )
                    )
        return session

    def discover(self, home: Path | None = None) -> list[dict]:
        home = home or Path(os.environ.get("CODEX_HOME", Path.home() / ".codex"))
        result = []
        for directory in (home / "sessions", home / "archived_sessions"):
            for path in sorted(directory.rglob("*.jsonl")):
                # Discovery reads metadata only; no transcript bodies or auth files.
                try:
                    with path.open(encoding="utf-8") as stream:
                        row = json.loads(stream.readline(128000))
                    p = row.get("payload", {})
                    if row.get("type") == "session_meta":
                        result.append(
                            {
                                "id": p.get("id", p.get("session_id", path.stem)),
                                "path": str(path),
                                "cwd": p.get("cwd"),
                                "timestamp": p.get("timestamp"),
                            }
                        )
                except (OSError, ValueError, AttributeError):
                    continue
        return result
