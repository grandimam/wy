from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, ConfigDict, Field, model_validator


class Model(BaseModel):
    model_config = ConfigDict(extra="forbid")


class Location(Model):
    file: str
    symbol: str = "<module>"
    start_line: int = Field(ge=1)
    end_line: int = Field(ge=1)

    @model_validator(mode="after")
    def ordered(self):
        if self.end_line < self.start_line:
            raise ValueError("end_line precedes start_line")
        return self


class Evidence(Location):
    id: str
    kind: Literal["code", "session", "diff"]
    excerpt: str
    snapshot_hash: str = ""
    event_id: str | None = None


class Event(Model):
    id: str
    kind: Literal["user", "assistant", "read", "search", "tool_call", "tool_output", "change", "test"]
    text: str
    source_line: int
    tool: str | None = None
    call_id: str | None = None
    files: list[str] = Field(default_factory=list)


class Session(Model):
    id: str
    path: str
    cwd: str | None = None
    format: str = "codex-rollout"
    events: list[Event] = Field(default_factory=list)
    warnings: list[str] = Field(default_factory=list)


class ReflectionContent(Model):
    """A retrospective assessment, never evidence of original intent."""

    decision_id: str
    assessment: Literal["keep", "revise", "insufficient_context"]
    rationale: str = Field(min_length=1, max_length=3000)
    evidence_ids: list[str] = Field(max_length=12)
    alternatives: list[str] = Field(max_length=6)
    assumptions: list[str] = Field(max_length=8)
    suggested_change: str = Field(max_length=3000)
    uncertainty: str = Field(min_length=1, max_length=2000)


class Reflection(ReflectionContent):
    request_id: str
    created_at: str
    agent: str
    model: str | None
    context: Literal["original_conversation", "separate_review"]
    identity_verification: Literal["self_reported"] = "self_reported"


class ReflectionResponse(Model):
    request_id: str
    review_id: str
    agent: str = Field(min_length=1, max_length=100)
    model: str | None = Field(max_length=100)
    context: Literal["original_conversation", "separate_review"]
    reflections: list[ReflectionContent] = Field(min_length=1, max_length=12)


class Decision(Model):
    id: str
    question: str
    category: Literal[
        "concurrency",
        "caching",
        "abstraction",
        "dependency",
        "configuration",
        "database",
        "failure-handling",
        "architecture",
    ]
    location: Location
    explanation: str
    provenance: Literal["recorded", "inferred", "unexplained"]
    evidence: list[Evidence]
    alternatives: list[str] = Field(default_factory=list)
    assumptions: list[str] = Field(default_factory=list)
    unresolved_questions: list[str] = Field(default_factory=list)
    snapshot_hash: str
    attribution: Literal["since-baseline", "unknown"] = "unknown"
    stale: bool = False
    reflections: list[Reflection] = Field(default_factory=list)

    @model_validator(mode="after")
    def supported(self):
        if self.provenance != "unexplained" and not self.evidence:
            raise ValueError("Supported explanations require evidence")
        if self.provenance == "recorded" and not any(e.kind == "session" for e in self.evidence):
            raise ValueError("Recorded explanations require session evidence")
        return self


class ChangedFile(Model):
    file: str
    hunks: list[Location]
    symbols: list[Location]
    added_lines: list[int]
    removed_line_count: int = Field(ge=0)


class Review(Model):
    schema_version: int = 1
    id: str
    root: str
    created_at: str
    head: str | None = None
    baseline_id: str | None = None
    session_id: str | None = None
    decisions: list[Decision] = Field(default_factory=list)
    changes: list[ChangedFile] = Field(default_factory=list)
    warnings: list[str] = Field(default_factory=list)
    file_hashes: dict[str, str] = Field(default_factory=dict)
    input_tokens: int = 0
    output_tokens: int = 0
    provider: str = "offline"


class Justification(Model):
    """Model output cannot set locations, attribution or fabricate evidence records."""

    explanation: str = Field(max_length=3000)
    provenance: Literal["inferred", "unexplained"]
    evidence_ids: list[str] = Field(max_length=12)
    alternatives: list[str] = Field(max_length=6)
    assumptions: list[str] = Field(max_length=8)
    unresolved_questions: list[str] = Field(max_length=8)
