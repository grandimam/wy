"""Follow citation edges without inventing a causal chain or silently moving anchors."""

from __future__ import annotations

from pathlib import Path

from wy.models import Decision, Evidence, Review
from wy.presentation import saved_session
from wy.security import digest, read_source, redact


def select_evidence(decision: Decision, target: str) -> Evidence:
    if target.isdecimal() and 1 <= int(target) <= len(decision.evidence):
        return decision.evidence[int(target) - 1]
    for evidence in decision.evidence:
        if evidence.id == target:
            return evidence
    raise ValueError("Use an evidence number or citation ID from this decision")


def inspect(review: Review, decision: Decision, evidence: Evidence) -> dict:
    result = {
        "review_id": review.id,
        "decision_id": decision.id,
        "session_id": review.session_id,
        "evidence": evidence.model_dump(mode="json"),
        "related_decisions": [
            {"number": n, "id": d.id, "question": d.question}
            for n, d in enumerate(review.decisions, 1)
            if d.id != decision.id and any(e.id == evidence.id for e in d.evidence)
        ],
    }
    if evidence.kind == "session":
        session = saved_session(review)
        events = session.events if session else []
        index = next((i for i, e in enumerate(events) if e.id == evidence.event_id), None)
        result["context"] = []
        result["linked_tool_events"] = []
        if index is not None:
            event = events[index]
            result["context"] = [e.model_dump() for e in events[max(0, index - 2):index + 3]]
            if event.call_id:
                result["linked_tool_events"] = [
                    e.model_dump() for e in events if e.call_id == event.call_id and e.id != event.id
                ]
        result["note"] = "Nearby events are chronological context, not proof of cause or authorship."
        return result
    raw = read_source(Path(review.root), evidence.file)
    result["current"] = None
    if raw is None:
        result["note"] = "Current source is missing or unavailable; the saved excerpt remains inspectable."
        return result
    current = redact(raw).splitlines()
    excerpt = evidence.excerpt.splitlines()
    unchanged = digest(raw) == evidence.snapshot_hash
    matches = []
    if excerpt:
        matches = [i + 1 for i in range(len(current) - len(excerpt) + 1) if current[i:i + len(excerpt)] == excerpt]
    if unchanged:
        line = evidence.start_line
        status = "unchanged"
    elif len(matches) == 1:
        line = matches[0]
        status = "unique_excerpt_match"
    else:
        line = min(evidence.start_line, max(1, len(current)))
        status = "ambiguous" if len(matches) > 1 else "excerpt_changed"
    start, end = max(1, line - 6), min(len(current), line + max(len(excerpt), 1) + 5)
    result["current"] = {
        "status": status,
        "file_unchanged": unchanged,
        "matching_lines": matches,
        "start_line": start,
        "end_line": end,
        "excerpt": "\n".join(current[start - 1:end]),
        "anchor_line": line,
    }
    result["note"] = (
        "Current file matches the review snapshot."
        if unchanged else
        "File changed since review. Matching text is a navigation aid, not revalidated evidence; the original citation is preserved."
    )
    return result
