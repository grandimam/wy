from __future__ import annotations

import re

from wy.models import Evidence, Session
from wy.security import digest


def code_evidence(file: str, text: str, line: int, hashes: dict[str, str]) -> Evidence:
    lines = text.splitlines()
    start, end = max(1, line - 2), min(len(lines), line + 2)
    return Evidence(
        id="code-" + digest(f"{file}:{start}:{end}")[:12],
        kind="code",
        file=file,
        start_line=start,
        end_line=end,
        excerpt="\n".join(lines[start - 1 : end]),
        snapshot_hash=hashes.get(file, ""),
    )


def retrieve(
    file: str, line: int, token: str, texts: dict[str, str], hashes: dict[str, str], session: Session | None
) -> list[Evidence]:
    evidence = [code_evidence(file, texts[file], line, hashes)]
    # Bounded lexical retrieval is transparent and includes competing conventions.
    patterns = [re.escape(token)]
    if token in {"ThreadPoolExecutor", "ProcessPoolExecutor", "asyncio", "threading"}:
        patterns += [r"requests\.(?:get|post|put)\(", r"sqlite3\.connect\(", r"asyncio\.", r"async def "]
    if token.lower() in {"redis", "lru_cache"}:
        patterns += [r"Redis\(", r"lru_cache", r"redis\."]
    pattern = re.compile("|".join(patterns), re.I)
    for other in sorted(texts):
        if other.endswith((".md", ".txt")):
            continue
        for number, source in enumerate(texts[other].splitlines(), 1):
            if pattern.search(source) and not source.lstrip().startswith(("#", "//", "from ", "import ")):
                item = code_evidence(other, texts[other], number, hashes)
                if item.id not in {e.id for e in evidence}:
                    evidence.append(item)
                if len(evidence) >= 7:
                    break
        if len(evidence) >= 7:
            break
    if session:
        basename = file.rsplit("/", 1)[-1]
        for event in session.events:
            if token.lower() in event.text.lower() and (file in event.text or basename in event.text):
                evidence.append(
                    Evidence(
                        id=f"session-{event.id}",
                        kind="session",
                        file=session.path,
                        start_line=event.source_line,
                        end_line=event.source_line,
                        excerpt=event.text,
                        event_id=event.id,
                    )
                )
                if len(evidence) >= 10:
                    break
    return evidence


def recorded_rationale(
    evidence: list[Evidence], session: Session | None, token: str, file: str
) -> Evidence | None:
    if not session:
        return None
    assistant_ids = {e.id for e in session.events if e.kind == "assistant"}
    for item in evidence:
        if item.kind != "session" or item.event_id not in assistant_ids:
            continue
        # Require a first-person completed choice, its subject, the affected file,
        # and a causal connective in the same sentence. No user requests, tool
        # output, source comments, proposals or arbitrary nearby assistant text.
        for sentence in re.split(r"(?<=[.!?])\s+|\n", item.excerpt):
            if (
                re.match(r"I (?:chose|used|introduced|added|selected|replaced)\b", sentence.strip())
                and re.search(r"\b(?:because|so that|in order to)\b", sentence)
                and token.lower() in sentence.lower()
                and file in sentence
                and not re.search(r"\b(?:not|never|didn't|don't|example|hypothetical)\b", sentence, re.I)
            ):
                return item.model_copy(update={"excerpt": sentence})
    return None
