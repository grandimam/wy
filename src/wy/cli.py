from __future__ import annotations

import json
import sqlite3
import subprocess
from functools import wraps
from pathlib import Path
from typing import Annotated

import typer

from wy import explorer, presentation, reflection, service, trace
from wy.ingestion.codex import CodexCollector
from wy.models import ReflectionResponse
from wy.provider import OllamaProvider
from wy.security import redact

app = typer.Typer(
    no_args_is_help=False, help="Open wy's decision workspace. Local and offline by default."
)
Repo = Annotated[Path, typer.Option("--repo", help="Repository directory")]
Json = Annotated[bool, typer.Option("--json", help="Emit structured JSON")]


def guarded(fn):
    @wraps(fn)
    def wrapped(*args, **kwargs):
        try:
            return fn(*args, **kwargs)
        except (ValueError, OSError, sqlite3.DatabaseError, subprocess.SubprocessError) as exc:
            # Never print Pydantic input dumps, provider bodies or raw credentials.
            message = "Invalid input or structured response" if hasattr(exc, "errors") else redact(str(exc))
            typer.echo(f"wy: {message}", err=True)
            raise typer.Exit(1) from None

    return wrapped


@app.callback(invoke_without_command=True)
@guarded
def main(ctx: typer.Context, repo: Repo = Path(".")):
    """Run wy with no command to open the interactive terminal workspace."""
    if ctx.invoked_subcommand is None:
        explorer.start(repo)


def emit(value, as_json: bool):
    data = value.model_dump(mode="json") if hasattr(value, "model_dump") else value
    if as_json:
        typer.echo(json.dumps(data, ensure_ascii=False, indent=2))
        return
    if isinstance(data, dict) and "decisions" in data and "root" in data:
        presentation.review_view(value)
    elif isinstance(data, dict) and "decisions" in data:
        for warning in data.get("warnings", []):
            typer.echo("Note: " + warning)
        for decision in data["decisions"]:
            emit(decision, False)
        typer.echo(
            f"\n{len(data['decisions'])} decisions · {data.get('provider', 'offline')} · tokens {data.get('input_tokens', 0)} in / {data.get('output_tokens', 0)} out"
        )
    elif isinstance(data, dict) and "question" in data:
        loc = data["location"]
        typer.echo(
            f"\n{data['question']} · {data['provenance'].title()}"
            + (" · STALE — re-review" if data.get("stale") else "")
        )
        typer.echo(f"{loc['file']}:{loc['start_line']} · {loc['symbol']} · {data['id']}")
        typer.echo(data["explanation"])
        for evidence in data["evidence"]:
            typer.echo(f"  Evidence [{evidence['id']}]: {evidence['file']}:{evidence['start_line']}")
        for label, key in (
            ("Alternative to investigate", "alternatives"),
            ("Assumption", "assumptions"),
            ("Open question", "unresolved_questions"),
        ):
            for item in data[key]:
                typer.echo(f"  {label}: {item}")
        for item in data.get("reflections", []):
            typer.echo(
                f"  Retrospective assessment: {item['assessment']} · {item['agent']} · identity self-reported"
            )
            typer.echo(f"    {item['rationale']}")
            typer.echo(f"    Uncertainty: {item['uncertainty']}")
            if item["suggested_change"]:
                typer.echo(f"    Suggested change: {item['suggested_change']}")
    elif isinstance(data, list):
        for item in data:
            if isinstance(item, dict) and "question" in item:
                emit(item, False)
            else:
                typer.echo(f"{item['id']}  {item.get('cwd') or '(workspace unknown)'}  {item['path']}")
        if not data:
            typer.echo("No results.")
    elif "answer" in data:
        typer.echo(data["answer"] + "\n\n" + data["uncertainty"])
        typer.echo("Evidence: " + ", ".join(data["evidence_ids"]))
    else:
        typer.echo(json.dumps(data, indent=2))


@app.command()
@guarded
def sessions(as_json: Json = False, codex_home: Annotated[Path | None, typer.Option()] = None):
    """List available Codex sessions (metadata only)."""
    emit(CodexCollector().discover(codex_home), as_json)


@app.command()
@guarded
def snapshot(repo: Repo = Path("."), as_json: Json = False):
    """Capture pre-session edits so a later review can isolate new changes."""
    result = service.snapshot(repo)
    emit({k: v for k, v in result.items() if k not in {"texts", "hashes"}}, as_json)


@app.command()
@guarded
def review(
    repo: Repo = Path("."),
    session: Annotated[str | None, typer.Option(help="Codex session ID or JSONL path")] = None,
    baseline: Annotated[str | None, typer.Option()] = None,
    base: Annotated[str | None, typer.Option(help="Git revision to compare against")] = None,
    diff: Annotated[
        Path | None, typer.Option(help="Unified diff whose target matches the working tree")
    ] = None,
    model: Annotated[bool, typer.Option(help="Explicitly use the configured model provider")] = False,
    as_json: Json = False,
):
    """Analyze changes, save decisions and refresh the editor cache."""
    session_path = None
    if session:
        if Path(session).is_file():
            session_path = Path(session)
        else:
            matches = [s for s in CodexCollector().discover() if s["id"] == session]
            if len(matches) != 1:
                raise ValueError("Session ID not found or ambiguous; pass an explicit JSONL path")
            session_path = Path(matches[0]["path"])
    result = service.review(
        repo, session_path, baseline, base, diff, OllamaProvider.from_env() if model else None
    )
    emit(result, as_json)


@app.command()
@guarded
def decisions(repo: Repo = Path("."), as_json: Json = False):
    """List cached decisions; no model is invoked."""
    emit(service.load_review(repo), as_json)


@app.command()
@guarded
def gaps(repo: Repo = Path("."), as_json: Json = False):
    """List unexplained decisions, unresolved questions and stale findings."""
    result = service.load_review(repo)
    if not as_json:
        presentation.review_view(result, gaps_only=True)
        return
    result.decisions = [
        d for d in result.decisions if d.provenance == "unexplained" or d.unresolved_questions or d.stale
    ]
    emit(result, as_json)


@app.command()
@guarded
def explain(
    target: str,
    repo: Repo = Path("."),
    as_json: Json = False,
    show_code: Annotated[bool, typer.Option(help="Include stored source excerpts")] = False,
):
    """Inspect a cached decision by number, file:line or decision ID."""
    result = service.load_review(repo)
    decision = service.select(result, target)
    if as_json:
        emit(decision, True)
    else:
        presentation.decision_view(decision, result, show_code=show_code)


@app.command()
@guarded
def session(
    repo: Repo = Path("."),
    as_json: Json = False,
    event: Annotated[str | None, typer.Option(help="Inspect a stored event, e.g. event-42")] = None,
):
    """Trace the current review to its saved Codex session and cited events."""
    result = service.load_review(repo)
    saved = presentation.saved_session(result)
    if saved is None:
        raise ValueError("No saved session available for this review; supply --session when reviewing")
    if as_json:
        if event:
            selected = next((e for e in saved.events if e.id == event), None)
            if selected is None:
                raise ValueError("Event not found in the saved session")
            emit(selected, True)
        else:
            emit(saved, True)
    else:
        presentation.session_view(result, saved, event)


@app.command()
@guarded
def explore(repo: Repo = Path(".")):
    """Browse decisions, code citations and Codex events interactively (read-only)."""
    explorer.start(repo)


@app.command()
@guarded
def evidence(target: str, citation: str, repo: Repo = Path("."), as_json: Json = False):
    """Trace a decision's citation by number or ID, including current source context."""
    result = service.load_review(repo)
    decision = service.select(result, target)
    data = trace.inspect(result, decision, trace.select_evidence(decision, citation))
    if as_json:
        emit(data, True)
    else:
        presentation.evidence_view(data)


@app.command()
@guarded
def focus(
    target: str,
    question: str,
    repo: Repo = Path("."),
    evidence: Annotated[
        list[str] | None, typer.Option(help="Additional evidence file:line; repeatable")
    ] = None,
    as_json: Json = False,
):
    """Add a question about existing code that automatic detection missed."""
    emit(reflection.focus(repo, target, question, evidence or []), as_json)


@app.command()
@guarded
def reflection_request(
    target: Annotated[str | None, typer.Argument(help="Optional decision ID or file:line")] = None,
    repo: Repo = Path("."),
    as_json: Json = False,
):
    """Prepare a structured self-review for the active coding conversation; no model call."""
    emit(reflection.request(repo, target), as_json)


@app.command()
@guarded
def record_reflection(response: Path, repo: Repo = Path("."), as_json: Json = False):
    """Validate an agent response and attach its retrospective assessment to the review."""
    with response.open("rb") as stream:
        raw = stream.read(1_000_001)
    if len(raw) > 1_000_000:
        raise ValueError("Reflection response exceeds the 1 MB input limit")
    parsed = ReflectionResponse.model_validate_json(raw)
    emit(reflection.record(repo, parsed), as_json)


@app.command()
@guarded
def ask(
    target: str,
    question: str,
    repo: Repo = Path("."),
    model: Annotated[bool, typer.Option()] = False,
    as_json: Json = False,
):
    """Investigate a decision using cached evidence, optionally with a model."""
    decision = service.select(service.load_review(repo), target)
    emit(service.ask(decision, question, OllamaProvider.from_env() if model else None), as_json)


if __name__ == "__main__":
    app()
