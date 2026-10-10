# Optional wy decision records

For significant architectural choices (new classes, abstractions, dependencies,
rejected alternatives), emit a concise public decision record in your assistant
response when making the decision. Do not expose private chain-of-thought.
Do not invent evidence, test outcomes, alternatives or references. Use "unknown"
when unavailable. Distinguish decision-time records from retrospective explanations.

Use the following marker and JSON shape so wy can find the record automatically:

WY_DECISION
```json
{
  "file": "src/session_repository.rs",
  "symbol": "SessionRepository",
  "decision": "Separate persistence from discovery",
  "reason": "Both JSONL and SQLite adapters need the same saved-history boundary",
  "requirement": "Support multiple coding tools",
  "alternatives": ["Inline persistence inside each adapter"],
  "tradeoffs": ["Adds an abstraction; avoids duplicated persistence behavior"],
  "evidence": ["Actual inspected file or message/tool event reference"],
  "related_edits": ["Actual edit reference if available"],
  "validation": "Not yet run",
  "timing": "decision-time"
}
```

Use repository-relative file paths. For multiple affected files, emit a record for
each significant boundary. Report validation results afterward; revise the record
if the implementation changes. Records are self-reported statements, not proof.
The example above is a format demonstration, not a decision to apply to this project.
