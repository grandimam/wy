"""Conservative input boundaries; no repository code is imported or executed."""

from __future__ import annotations

import hashlib
import re
from pathlib import Path

MAX_FILE_BYTES = 512_000
MAX_REPO_BYTES = 12_000_000
TEXT_SUFFIXES = {
    ".py",
    ".ts",
    ".tsx",
    ".js",
    ".jsx",
    ".json",
    ".toml",
    ".yaml",
    ".yml",
    ".sql",
    ".md",
    ".txt",
    ".ini",
    ".cfg",
    ".go",
    ".rs",
    ".java",
}
EXCLUDED = {".git", ".wy", ".venv", "venv", "node_modules", "dist", "build", "vendor", "__pycache__"}


def safe_relative(file: str) -> bool:
    p = Path(file)
    return not p.is_absolute() and ".." not in p.parts and "\\" not in file and bool(p.parts)


def allowed(file: str) -> bool:
    p = Path(file)
    lower = p.name.lower()
    return (
        safe_relative(file)
        and not any(part in EXCLUDED for part in p.parts)
        and not any(
            word in lower for word in (".env", "credential", "secret", "id_rsa", "id_ed25519", "auth.json")
        )
        and p.suffix.lower() in TEXT_SUFFIXES
    )


def digest(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def redact(text: str) -> str:
    text = re.sub(
        r"-----BEGIN [^-]*PRIVATE KEY-----[\s\S]*?-----END [^-]*PRIVATE KEY-----",
        lambda match: "[REDACTED PRIVATE KEY]" + "\n" * match.group(0).count("\n"),
        text,
    )
    text = re.sub(
        r"\b(?:sk-[A-Za-z0-9_-]{12,}|gh[pousr]_[A-Za-z0-9_]{16,}|AKIA[A-Z0-9]{16})\b", "[REDACTED]", text
    )
    text = re.sub(
        r"(?i)(\b(?:api[_-]?key|password|passwd|secret|access[_-]?token|authorization)\b[\"']?\s*[:=]\s*)(?:[\"'][^\"'\n]*[\"']|[^\s,;}]+)",
        r"\1[REDACTED]",
        text,
    )
    text = re.sub(r"(?i)\bBearer\s+[A-Za-z0-9._~+/-]+=*", "Bearer [REDACTED]", text)
    text = re.sub(r"(\w+://)[^\s/@:]+:[^\s/@]+@", r"\1[REDACTED]@", text)
    # Strip terminal control characters, retaining newlines and tabs.
    return re.sub(r"[\x00-\x08\x0b-\x1f\x7f]", "", text)


def read_source(root: Path, file: str) -> str | None:
    if not allowed(file):
        return None
    path = root / file
    # Refuse all symlink components, even symlinks pointing back into the repo.
    if any(p.is_symlink() for p in [path, *path.parents] if p != root.parent):
        return None
    try:
        path.resolve().relative_to(root.resolve())
        if not path.is_file() or path.stat().st_size > MAX_FILE_BYTES:
            return None
        raw = path.read_bytes()
        if b"\0" in raw:
            return None
        return raw.decode("utf-8")
    except (OSError, ValueError):
        return None
