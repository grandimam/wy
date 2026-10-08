from __future__ import annotations

import ast
import difflib
import json
import re
import subprocess
from dataclasses import dataclass, field
from pathlib import Path

from wy.models import Location
from wy.security import MAX_REPO_BYTES, allowed, digest, read_source, redact


def git(root: Path, *args: str, check: bool = True) -> str:
    result = subprocess.run(
        ["git", "--no-pager", "-c", "core.quotePath=false", "-c", "core.fsmonitor=false", *args],
        cwd=root,
        capture_output=True,
        encoding="utf-8",
        errors="replace",
        timeout=30,
    )
    if check and result.returncode:
        raise ValueError("Git operation failed: " + redact(result.stderr.strip()))
    return result.stdout


def repo_root(path: Path) -> Path:
    return Path(git(path, "rev-parse", "--show-toplevel").strip()).resolve()


def head(root: Path) -> str | None:
    return git(root, "rev-parse", "--verify", "HEAD", check=False).strip() or None


def sources(root: Path) -> tuple[dict[str, str], dict[str, str], list[str]]:
    names = git(root, "ls-files", "-z", "--cached", "--others", "--exclude-standard").split("\0")
    texts, hashes, warnings = {}, {}, []
    total = 0
    for name in sorted(set(names)):
        if not name or not allowed(name):
            continue
        text = read_source(root, name)
        if text is None:
            warnings.append(f"Skipped unreadable, binary, symlinked or oversized file: {name}")
            continue
        total += len(text.encode())
        if total > MAX_REPO_BYTES:
            warnings.append("Repository context truncated at 12 MB")
            break
        hashes[name] = digest(text)
        texts[name] = redact(text)
    return texts, hashes, warnings


def committed_sources(root: Path, revision: str) -> dict[str, str]:
    # Resolve to a hash first: user input never becomes a Git option or pathspec.
    commit = git(root, "rev-parse", "--verify", "--end-of-options", revision + "^{commit}").strip()
    records = git(root, "ls-tree", "-r", "-z", "-l", commit).split("\0")
    result, total = {}, 0
    for record in records:
        if not record or "\t" not in record:
            continue
        metadata, name = record.split("\t", 1)
        mode, kind, oid, size = metadata.split()
        if kind != "blob" or mode == "120000" or not allowed(name) or int(size) > 512000:
            continue
        total += int(size)
        if total > MAX_REPO_BYTES:
            raise ValueError("Base revision exceeds the 12 MB source limit; narrow the repository")
        text = git(root, "cat-file", "blob", oid)
        if "\0" not in text:
            result[name] = redact(text)
    return result


@dataclass
class Change:
    file: str
    additions: dict[int, str] = field(default_factory=dict)
    removed: list[str] = field(default_factory=list)
    hunks: list[tuple[int, int]] = field(default_factory=list)


def compare(before: dict[str, str], after: dict[str, str]) -> list[Change]:
    changes = []
    for name in sorted(before.keys() | after.keys()):
        old, new = before.get(name, "").splitlines(), after.get(name, "").splitlines()
        change = Change(name)
        for tag, a, b, c, d in difflib.SequenceMatcher(None, old, new, autojunk=False).get_opcodes():
            if tag == "equal":
                continue
            change.additions.update({i + 1: new[i] for i in range(c, d)})
            change.removed.extend(old[a:b])
            change.hunks.append((c + 1, max(c + 1, d)))
        if change.hunks:
            changes.append(change)
    return changes


def parse_diff(text: str) -> list[Change]:
    """Read unified Git diffs without applying them or invoking patch programs."""
    changes, current, line_no, in_hunk = [], None, 0, False
    for line in text.splitlines():
        if line.startswith("diff --git "):
            current, in_hunk = None, False
        elif not in_hunk and line.startswith("+++ "):
            name = line[4:].split("\t", 1)[0]
            if name.startswith('"'):
                try:
                    name = json.loads(name)
                except ValueError:
                    continue
            if name.startswith("b/"):
                name = name[2:]
            current = Change(name) if allowed(name) else None
            if current:
                changes.append(current)
        elif line.startswith("@@ "):
            match = re.match(r"@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@", line)
            if match and current:
                line_no = int(match[1])
                count = int(match[2] or 1)
                current.hunks.append((max(1, line_no), max(1, line_no + count - 1)))
                in_hunk = True
        elif current and in_hunk:
            if line.startswith("+"):
                current.additions[line_no] = redact(line[1:])
                line_no += 1
            elif line.startswith("-"):
                current.removed.append(redact(line[1:]))
            elif line.startswith(" "):
                line_no += 1
    return changes


def changed_symbols(file: str, text: str, lines: set[int]) -> list[Location]:
    if not file.endswith(".py"):
        return []
    try:
        tree = ast.parse(text)
    except (SyntaxError, ValueError, RecursionError):
        return []
    found = []

    def walk(node, parents):
        for child in ast.iter_child_nodes(node):
            if isinstance(child, (ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef)):
                end = child.end_lineno or child.lineno
                name = ".".join([*parents, child.name])
                if any(child.lineno <= line <= end for line in lines):
                    found.append(Location(file=file, symbol=name, start_line=child.lineno, end_line=end))
                walk(child, [*parents, child.name])
            else:
                walk(child, parents)

    walk(tree, [])
    return found


def location_for(file: str, text: str, line: int) -> Location:
    symbols = changed_symbols(file, text, {line})
    if symbols:
        return min(symbols, key=lambda s: s.end_line - s.start_line)
    return Location(file=file, start_line=line, end_line=line)
