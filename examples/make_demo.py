"""Create a NEW throwaway Git repository; sample application code is never executed."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

from wy.service import snapshot


def main():
    if len(sys.argv) != 2:
        raise SystemExit("Usage: python examples/make_demo.py /tmp/wy-demo")
    root = Path(sys.argv[1]).resolve()
    if root.exists():
        raise SystemExit("Destination already exists; choose a new path (nothing was overwritten).")
    root.mkdir(parents=True)
    fixtures = Path(__file__).parent / "threaded-worker"
    (root / "worker.py").write_text((fixtures / "before.py").read_text())
    (root / ".gitignore").write_text(".wy/\n")
    subprocess.run(["git", "init", "-q", str(root)], check=True)
    subprocess.run(["git", "-C", str(root), "add", "."], check=True)
    subprocess.run(
        [
            "git",
            "-C",
            str(root),
            "-c",
            "user.name=wy demo",
            "-c",
            "user.email=demo@example.invalid",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "-qm",
            "Before agent implementation",
        ],
        check=True,
    )
    baseline = snapshot(root)
    (root / "worker.py").write_text((fixtures / "after.py").read_text())
    transcript = [
        {"type": "session_meta", "payload": {"id": "demo-session", "cwd": str(root)}},
        {
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "user",
                "content": [
                    {
                        "type": "input_text",
                        "text": "Speed up batch downloads while preserving the synchronous client.",
                    }
                ],
            },
        },
        {
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "assistant",
                "phase": "final_answer",
                "content": [
                    {
                        "type": "output_text",
                        "text": "I chose ThreadPoolExecutor in worker.py because the existing fetch function uses the synchronous requests client.",
                    }
                ],
            },
        },
    ]
    (root / ".wy" / "demo-session.jsonl").write_text("\n".join(json.dumps(row) for row in transcript) + "\n")
    print(
        json.dumps(
            {
                "repo": str(root),
                "baseline": baseline["id"],
                "session": str(root / ".wy" / "demo-session.jsonl"),
            },
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
