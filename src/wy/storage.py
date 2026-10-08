from __future__ import annotations

import json
import os
import sqlite3
import tempfile
from pathlib import Path

from wy.models import Review


class Store:
    def __init__(self, root: Path):
        self.root = root.resolve()
        self.directory = self.root / ".wy"
        if self.directory.is_symlink():
            raise ValueError("Refusing symlinked .wy storage")
        self.directory.mkdir(mode=0o700, exist_ok=True)
        self.database = self.directory / "wy.sqlite3"
        if self.database.is_symlink():
            raise ValueError("Refusing symlinked database")
        # Create privately before SQLite opens it.
        fd = os.open(self.database, os.O_CREAT | os.O_WRONLY, 0o600)
        os.close(fd)
        with self.connect() as db:
            db.execute(
                "CREATE TABLE IF NOT EXISTS artifacts (kind TEXT, id TEXT, data TEXT, PRIMARY KEY(kind,id))"
            )
            db.execute("PRAGMA user_version=1")

    def connect(self):
        return sqlite3.connect(self.database, timeout=10)

    def put(self, kind: str, id: str, data: dict):
        with self.connect() as db:
            db.execute("INSERT OR REPLACE INTO artifacts VALUES (?,?,?)", (kind, id, json.dumps(data)))

    def get(self, kind: str, id: str) -> dict:
        with self.connect() as db:
            row = db.execute("SELECT data FROM artifacts WHERE kind=? AND id=?", (kind, id)).fetchone()
        if not row:
            raise ValueError(f"No {kind} named {id}")
        return json.loads(row[0])

    def save_review(self, review: Review):
        data = review.model_dump(mode="json")
        self.put("review", review.id, data)
        self.put("review", "latest", data)
        # The extension only reads this atomic, versioned presentation cache.
        fd, path = tempfile.mkstemp(prefix="review-", suffix=".tmp", dir=self.directory)
        try:
            with os.fdopen(fd, "w", encoding="utf-8") as stream:
                json.dump(data, stream, ensure_ascii=False, indent=2)
            os.replace(path, self.directory / "review.json")
        finally:
            if os.path.exists(path):
                os.unlink(path)

    def latest(self) -> Review:
        return Review.model_validate(self.get("review", "latest"))
