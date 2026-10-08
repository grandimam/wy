"""Structural navigation of source text without executing application code."""

from __future__ import annotations

import ast
import json
import re
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class Symbol:
    name: str
    kind: str
    start: int
    end: int
    depth: int = 0


def outline(file: str, text: str) -> list[Symbol]:
    suffix = Path(file).suffix.lower()
    if suffix == ".md":
        found, fence = [], False
        for number, line in enumerate(text.splitlines(), 1):
            if line.lstrip().startswith(("```", "~~~")):
                fence = not fence
            if not fence and (match := re.match(r"^(#{1,6})\s+(.+)", line)):
                found.append(Symbol(match[2], "heading", number, number, len(match[1]) - 1))
        return [Symbol(s.name, s.kind, s.start,
                       next((other.start - 1 for other in found[i + 1:] if other.depth <= s.depth), len(text.splitlines())), s.depth)
                for i, s in enumerate(found)]
    if suffix == ".py":
        try:
            tree = ast.parse(text)
        except (SyntaxError, ValueError, RecursionError):
            return []
        found = []
        stack = [(tree, ())]
        while stack and len(found) < 500:
            node, parents = stack.pop()
            if isinstance(node, (ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef)):
                start = min([node.lineno, *[d.lineno for d in node.decorator_list]])
                found.append(Symbol(".".join((*parents, node.name)),
                                    "class" if isinstance(node, ast.ClassDef) else "function",
                                    start, node.end_lineno or node.lineno, len(parents)))
                parents = (*parents, node.name)
            stack.extend((child, parents) for child in reversed(list(ast.iter_child_nodes(node))))
        return found
    if suffix == ".json":
        try:
            json.loads(text)
            decoder = json.JSONDecoder()
            found, parents = [], []
            for match in re.finditer(r'"(?:\\.|[^"\\])*"', text):
                after = match.end()
                while after < len(text) and text[after].isspace():
                    after += 1
                if after >= len(text) or text[after] != ":":
                    continue
                value_start = after + 1
                while value_start < len(text) and text[value_start].isspace():
                    value_start += 1
                _, value_end = decoder.raw_decode(text, value_start)
                while parents and parents[-1][1] <= match.start():
                    parents.pop()
                name = match.group()
                found.append(Symbol(".".join([*[p[0] for p in parents], name]), "key",
                                    text.count("\n", 0, match.start()) + 1,
                                    text.count("\n", 0, value_end) + 1, len(parents)))
                if len(found) == 500:
                    break
                parents.append((name, value_end))
            return found
        except (ValueError, RecursionError):
            return []
    return []


def window(text: str, symbols: list[Symbol], line: int, *, span: int = 1, limit: int = 100) -> tuple[int, int, str]:
    """Prefer a small enclosing symbol; bound large/unknown structures around the target."""
    count = max(1, len(text.splitlines()))
    line = max(1, min(line, count))
    enclosing = [s for s in symbols if s.start <= line <= s.end]
    symbol = min(enclosing, key=lambda s: s.end - s.start) if enclosing else None
    start = max(1, line - 6)
    end = min(count, line + min(span, limit // 2) + 6)
    if symbol and symbol.end - symbol.start + 1 <= limit:
        start, end = min(start, symbol.start), max(end, symbol.end)
    if end - start + 1 > limit:
        start, end = max(1, line - 8), min(count, line + limit - 9)
    label = f"{symbol.kind} {symbol.name}" if symbol else "surrounding lines"
    return start, end, label
