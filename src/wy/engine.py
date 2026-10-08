from __future__ import annotations

import ast
import re
from dataclasses import dataclass

from wy.evidence import recorded_rationale, retrieve
from wy.models import Decision, Session
from wy.repository import Change, location_for
from wy.security import digest


@dataclass(frozen=True)
class Rule:
    category: str
    pattern: str
    token: str
    question: str
    alternatives: tuple[str, ...]
    gap: str


RULES = [
    Rule(
        "concurrency",
        r"\bThreadPoolExecutor\s*\(",
        "ThreadPoolExecutor",
        "Why use ThreadPoolExecutor?",
        (
            "asyncio with nonblocking clients",
            "Sequential execution",
            "ProcessPoolExecutor for CPU-bound work",
        ),
        "What workload and measurements justify the worker count?",
    ),
    Rule(
        "concurrency",
        r"\bProcessPoolExecutor\s*\(",
        "ProcessPoolExecutor",
        "Why use ProcessPoolExecutor?",
        ("Threads for blocking I/O", "Sequential execution"),
        "Does the work justify process startup and serialization costs?",
    ),
    Rule(
        "concurrency",
        r"\basyncio\.(?:gather|create_task|TaskGroup)\s*\(",
        "asyncio",
        "Why introduce asynchronous concurrency?",
        ("Sequential awaits", "Threads for blocking clients"),
        "How are cancellation, task failures and concurrency limits handled?",
    ),
    Rule(
        "caching",
        r"\b(?:redis\.(?:Redis|from_url)|Redis)\s*\(",
        "redis",
        "Why use Redis?",
        ("An in-process cache", "Database-backed storage"),
        "Which sharing, durability or eviction requirements justify an external service?",
    ),
    Rule(
        "caching",
        r"@(?:functools\.)?lru_cache\b",
        "lru_cache",
        "Why cache results in this process?",
        ("No caching", "A shared external cache"),
        "What invalidates cached results, and is per-process consistency sufficient?",
    ),
    Rule(
        "failure-handling",
        r"^\s*except\s+(?:Exception|BaseException)\b",
        "except",
        "Why catch this broad class of failures?",
        ("Catch specific recoverable exceptions", "Propagate errors to the caller"),
        "Which failures are safe to recover from, and which must remain visible?",
    ),
    Rule(
        "failure-handling",
        r"(?:@retry\b|\bRetry\s*\(|\bstop_after_attempt\s*\()",
        "retry",
        "Why introduce automatic retries?",
        ("Fail immediately", "Queue work for later retry"),
        "Are operations idempotent, and are retry limits and backoff justified?",
    ),
    Rule(
        "database",
        r"\b(?:CREATE TABLE|ALTER TABLE|CREATE (?:UNIQUE )?INDEX|op\.(?:add_column|create_table|create_index|drop_column))\b",
        "schema",
        "Why change the database schema?",
        ("Preserve the current schema", "Use a staged additive migration"),
        "What query or data requirement motivates this change, and how is rollback handled?",
    ),
    Rule(
        "abstraction",
        r"^\s*class\s+\w+(?:Repository|Factory|Adapter|Service|Strategy|Protocol)\b|^\s*class\s+\w+\s*\([^)]*\b(?:ABC|Protocol)\b",
        "abstraction",
        "Why introduce this abstraction?",
        ("Keep the behavior in an existing function", "Use composition with a small helper"),
        "Which independent callers or implementations require this boundary?",
    ),
]


def syntax_matches(file: str, text: str, line: int, rule: Rule) -> bool:
    if not file.endswith(".py") or rule.category == "database":
        return True
    try:
        tree = ast.parse(text)
    except (SyntaxError, ValueError, RecursionError):
        return False
    types = (
        (ast.ClassDef,)
        if rule.category == "abstraction"
        else (ast.ExceptHandler,)
        if rule.token == "except"
        else (ast.Call,)
    )
    return any(isinstance(node, types) and node.lineno == line for node in ast.walk(tree))


def synchronous_call_in_excerpt(item, texts: dict[str, str]) -> bool:
    if item.kind != "code" or not item.file.endswith(".py"):
        return False
    try:
        tree = ast.parse(texts[item.file])
    except (SyntaxError, ValueError, RecursionError):
        return False
    for node in ast.walk(tree):
        if isinstance(node, ast.Call) and item.start_line <= node.lineno <= item.end_line:
            name = ast.unparse(node.func)
            if name in {"requests.get", "requests.post", "requests.put", "sqlite3.connect"}:
                return True
    return False


def candidates(changes: list[Change], texts: dict[str, str]):
    seen = set()
    for change in changes:
        if change.file not in texts:
            continue
        for line, source in sorted(change.additions.items()):
            if source.lstrip().startswith(("#", "//", "import ", "from ")):
                continue
            for rule in RULES:
                if not re.search(rule.pattern, source, re.I if rule.category == "database" else 0):
                    continue
                if not syntax_matches(change.file, texts[change.file], line, rule):
                    continue
                # Do not annotate a mere moved unchanged choice.
                if any(source.strip() == old.strip() for old in change.removed):
                    continue
                loc = location_for(change.file, texts[change.file], line)
                key = (change.file, loc.symbol, rule.question)
                if key in seen:
                    continue
                seen.add(key)
                token = rule.token
                if token == "abstraction":
                    match = re.search(r"class\s+(\w+)", source)
                    token = match[1] if match else token
                yield change, line, rule, token
        name = change.file.rsplit("/", 1)[-1]
        if (
            name in {"pyproject.toml", "package.json", "requirements.txt", "Cargo.toml", "go.mod"}
            and change.additions
        ):
            # Require dependency-shaped lines; avoid project metadata annotations.
            matching = [
                (n, s)
                for n, s in change.additions.items()
                if re.search(r'["\w][\w.-]+\s*(?:[<>=~^]+\s*\d|["\']\s*:\s*["\'][~^]?\d)', s)
            ]
            if matching:
                yield (
                    change,
                    matching[0][0],
                    Rule(
                        "dependency",
                        "",
                        "dependency",
                        "Why change these dependencies?",
                        ("Use existing dependencies", "Implement a smaller local helper"),
                        "Which capability and compatibility constraints justify these versions?",
                    ),
                    "dependency",
                )
        elif (
            name.endswith((".yaml", ".yml", ".ini", ".cfg")) or name in {"config.toml", "settings.toml"}
        ) and change.additions:
            matching = [
                (n, s)
                for n, s in change.additions.items()
                if re.search(r"\b(?:timeout|retries|max_workers|pool_size|replicas|concurrency)\s*[:=]", s)
            ]
            if matching:
                yield (
                    change,
                    matching[0][0],
                    Rule(
                        "configuration",
                        "",
                        "configuration",
                        "Why choose these operational limits?",
                        ("Use the existing defaults", "Derive limits from measured capacity"),
                        "What measurements support these limits?",
                    ),
                    "configuration",
                )


def analyze(
    changes: list[Change],
    texts: dict[str, str],
    hashes: dict[str, str],
    session: Session | list[Session] | None = None,
    baseline: bool = False,
    limit: int = 12,
) -> list[Decision]:
    decisions = []
    for change, line, rule, token in candidates(changes, texts):
        evidence = retrieve(change.file, line, token, texts, hashes, session)
        recorded = recorded_rationale(evidence, session, token, change.file)
        provenance = "unexplained"
        explanation = "The change establishes this choice, but the available evidence does not establish why it was selected."
        assumptions = []
        gaps = [rule.gap]
        if recorded:
            provenance = "recorded"
            explanation = f"The assistant explicitly stated: “{recorded.excerpt}”"
            evidence = [recorded if e.id == recorded.id else e for e in evidence]
            assumptions.append(
                "This is an observable assistant statement, not proof that its justification is correct."
            )
        elif token == "ThreadPoolExecutor":
            blocking = [e for e in evidence if synchronous_call_in_excerpt(e, texts)]
            if blocking:
                provenance = "inferred"
                refs = ", ".join(f"{e.file}:{e.start_line}" for e in blocking)
                explanation = f"A plausible reason is to overlap blocking I/O: synchronous client calls appear in {refs}. Whether those calls belong to the submitted tasks needs verification. This is a hypothesis, not the agent's recorded intent."
                assumptions.append(
                    "The executor's workload reaches these synchronous client calls and is safe to run concurrently."
                )
        if token in {"ThreadPoolExecutor", "ProcessPoolExecutor"}:
            if any(e.kind == "code" and re.search(r"\basync def\b|asyncio\.", e.excerpt) for e in evidence):
                gaps.append(
                    "Async code also exists nearby; verify which concurrency convention applies to this workload."
                )
        # A matching implementation elsewhere is evidence of occurrence, not an asserted universal convention.
        peers = [e for e in evidence if e.kind == "code" and e.file != change.file and token in e.excerpt]
        if peers:
            gaps.append(
                "Similar code exists in "
                + ", ".join(sorted({e.file for e in peers}))
                + "; should this change follow the same convention?"
            )
        loc = location_for(change.file, texts[change.file], line)
        # Anchor the significant expression, retaining its enclosing symbol name.
        loc = loc.model_copy(update={"start_line": line, "end_line": min(loc.end_line, line + 3)})
        decisions.append(
            Decision(
                id="decision-" + digest(f"{change.file}:{loc.symbol}:{rule.question}:{line}")[:12],
                question=rule.question,
                category=rule.category,
                location=loc,
                explanation=explanation,
                provenance=provenance,
                evidence=evidence,
                alternatives=list(rule.alternatives),
                assumptions=assumptions,
                unresolved_questions=gaps,
                snapshot_hash=hashes.get(change.file, ""),
                attribution="since-baseline" if baseline else "unknown",
            )
        )
        if len(decisions) >= limit:
            break
    return decisions
