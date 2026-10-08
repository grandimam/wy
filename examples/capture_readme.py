"""Capture the real terminal UI using only the bundled synthetic demo.

Run from the project root: uv run python examples/capture_readme.py
"""

from __future__ import annotations

import asyncio
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

# Screenshots should use the application's normal color palette even in CI.
os.environ.pop("NO_COLOR", None)

from wy import service  # noqa: E402
from wy.explorer import Explorer  # noqa: E402


async def capture(root: Path, assets: Path):
    review = service.load_review(root)
    app = Explorer(review)
    async with app.run_test(size=(144, 44)) as pilot:
        await pilot.pause()
        decision = review.decisions[0]
        for filename, route in (
            ("workspace.svg", ("decision", decision.id, None)),
            ("evidence.svg", ("evidence", decision.id, decision.evidence[0].id)),
        ):
            app.navigate(route)
            await pilot.pause()
            app.save_screenshot(filename, path=str(assets))


def main():
    project = Path(__file__).resolve().parents[1]
    assets = project / "docs" / "assets"
    assets.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="wy-readme-") as temporary:
        root = Path(temporary) / "demo"
        output = subprocess.check_output(
            [sys.executable, str(project / "examples" / "make_demo.py"), str(root)],
            text=True,
        )
        demo = json.loads(output)
        service.review(root, Path(demo["session"]), baseline_id=demo["baseline"])
        asyncio.run(capture(root, assets))
    print(f"Saved workspace.svg and evidence.svg in {assets}")


if __name__ == "__main__":
    main()
