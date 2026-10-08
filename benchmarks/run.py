"""Deterministic, synthetic regression benchmark; no model calls or monetary costs."""

import json
from pathlib import Path

from wy.engine import analyze
from wy.models import Event, Session
from wy.repository import compare
from wy.security import digest


def evaluate():
    cases = json.loads((Path(__file__).parent / "cases.json").read_text())
    correct = predicted = expected = citations = valid_citations = unsupported = relevant = 0
    details = []
    for case in cases:
        session = None
        if "rationale" in case:
            session = Session(
                id=case["name"],
                path="benchmark.jsonl",
                events=[Event(id="e1", kind="assistant", text=case["rationale"], source_line=1)],
            )
        decisions = analyze(
            compare(case["before"], case["after"]),
            case["after"],
            {f: digest(t) for f, t in case["after"].items()},
            session,
        )
        gold = {(e["file"], e["category"]): e for e in case["expected"]}
        predicted += len(decisions)
        expected += len(gold)
        for d in decisions:
            label = gold.get((d.location.file, d.category))
            correct += label is not None
            relevant += bool(label and label["provenance"] == d.provenance)
            unsupported += bool(
                d.provenance != "unexplained" and (not label or d.provenance != label["provenance"])
            )
            for e in d.evidence:
                citations += 1
                if e.kind == "code":
                    lines = case["after"][e.file].splitlines()
                    valid_citations += e.excerpt == "\n".join(lines[e.start_line - 1 : e.end_line])
                else:
                    valid_citations += e.excerpt in case.get("rationale", "")
        details.append(
            {
                "case": case["name"],
                "expected": len(gold),
                "found": len(decisions),
                "provenance": [d.provenance for d in decisions],
            }
        )
    return {
        "suite": "synthetic-offline-v1",
        "cases": len(cases),
        "decisions": predicted,
        "decision_precision": correct / predicted if predicted else None,
        "decision_recall": correct / expected if expected else None,
        "evidence_citation_accuracy": valid_citations / citations if citations else None,
        "unsupported_explanation_rate": unsupported / predicted if predicted else None,
        "provenance_label_accuracy": relevant / predicted if predicted else None,
        "annotation_usefulness": "Not human-rated; expected decision labels are a regression proxy only.",
        "explanation_relevance": "Requires human evaluation; provenance label accuracy is not semantic relevance.",
        "stale_annotation_handling": "Covered by Python and extension automated tests; run both suites.",
        "tokens_per_session": 0,
        "model_cost": 0,
        "results": details,
    }


if __name__ == "__main__":
    print(json.dumps(evaluate(), indent=2))
