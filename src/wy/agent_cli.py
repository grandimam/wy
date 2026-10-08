"""Structured, opt-in reasoning through the user's installed agent CLI."""

from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import tempfile
import threading
import time
from pathlib import Path

from wy.security import redact


def available() -> list[str]:
    return [name for name in ("codex", "claude") if shutil.which(name)]


def command(agent: str, directory: Path, schema: dict) -> list[str]:
    executable = shutil.which(agent) if agent in {"codex", "claude"} else None
    if not executable:
        raise ValueError(f"Install and sign in to the {agent} CLI before using it for reasoning")
    if agent == "codex":
        schema_path = directory / "response-schema.json"
        schema_path.write_text(json.dumps(schema))
        args = [executable, "exec", "--ignore-user-config", "--ignore-rules", "--ephemeral",
                "--sandbox", "read-only", "--skip-git-repo-check", "--color", "never",
                "--output-schema", str(schema_path), "--output-last-message", str(directory / "response.json")]
        for feature in ("shell_tool", "unified_exec", "multi_agent", "hooks", "apps", "plugins",
                        "browser_use", "computer_use", "image_generation", "skill_search"):
            args += ["--disable", feature]
        args += ["--enable", "skip_host_skill_discovery", "--config", 'web_search="disabled"', "-"]
        return args
    return [executable, "--print", "--output-format", "json", "--json-schema", json.dumps(schema),
            "--tools", "", "--strict-mcp-config", "--mcp-config", '{"mcpServers":{}}',
            "--setting-sources", "", "--settings", '{"disableAllHooks":true}',
            "--disable-slash-commands", "--no-chrome", "--no-session-persistence"]


def stop(process: subprocess.Popen):
    if process.poll() is not None:
        return
    try:
        if os.name == "posix":
            os.killpg(process.pid, signal.SIGTERM)
        else:
            process.terminate()
        process.wait(timeout=2)
    except (ProcessLookupError, subprocess.TimeoutExpired):
        if process.poll() is None:
            if os.name == "posix":
                os.killpg(process.pid, signal.SIGKILL)
            else:
                process.kill()
            process.wait()


def invoke(agent: str, prompt: str, schema: dict, cancel: threading.Event | None = None,
           timeout: float = 240) -> dict:
    with tempfile.TemporaryDirectory(prefix="wy-reason-") as temporary:
        directory = Path(temporary)
        args = command(agent, directory, schema)
        env = os.environ.copy()
        # History home overrides are often set for another project. Authentication
        # stays with the CLI's configured user; no repository environment is read.
        env["NO_COLOR"] = "1"
        try:
            process = subprocess.Popen(args, cwd=directory, env=env, stdin=subprocess.PIPE,
                                       stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                       text=True, start_new_session=True)
        except OSError as exc:
            raise ValueError(f"Could not start {agent}; check its installation") from exc
        started, initial = time.monotonic(), True
        try:
            while True:
                if cancel and cancel.is_set():
                    raise ValueError("Reasoning cancelled")
                if time.monotonic() - started > timeout:
                    raise ValueError(f"{agent} exceeded the {int(timeout)} second reasoning timeout")
                try:
                    stdout, _stderr = process.communicate(input=prompt if initial else None, timeout=0.2)
                    break
                except subprocess.TimeoutExpired:
                    initial = False
            if process.returncode:
                raise ValueError(f"{agent} reasoning failed (exit {process.returncode}); check CLI sign-in and connectivity")
            if agent == "codex":
                path = directory / "response.json"
                if not path.exists() or path.stat().st_size > 200_000:
                    raise ValueError("Codex did not return a bounded structured answer")
                raw = path.read_text()
            else:
                if len(stdout) > 1_000_000:
                    raise ValueError("Claude response exceeded the output limit")
                envelope = json.loads(stdout)
                if envelope.get("is_error"):
                    raise ValueError("Claude returned an error; check CLI sign-in and connectivity")
                structured = envelope.get("structured_output")
                raw = json.dumps(structured) if isinstance(structured, dict) else envelope.get("result", "")
            result = json.loads(raw)
            if not isinstance(result, dict):
                raise ValueError("Agent did not return a structured object")
            def clean(value):
                if isinstance(value, str):
                    return redact(value)
                if isinstance(value, list):
                    return [clean(item) for item in value]
                if isinstance(value, dict):
                    return {key: clean(item) for key, item in value.items()}
                return value
            return clean(result)
        except (json.JSONDecodeError, TypeError) as exc:
            raise ValueError("Agent returned an invalid structured answer") from exc
        finally:
            stop(process)
