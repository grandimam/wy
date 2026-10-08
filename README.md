# wy

**AI writes the code. wy helps you understand the decisions behind it.**

wy finds consequential choices in a Git change, connects them to repository and available Codex evidence, and makes them discoverable through a CLI and VS Code CodeLens. It never inserts explanatory comments into application source.

This is a functional, precision-oriented MVP. It works offline without a model or an account. Optional structured model analysis can enrich the detected decisions.

```text
Why use ThreadPoolExecutor? · Recorded · View evidence

11     with ThreadPoolExecutor(max_workers=8) as pool:
12         return list(pool.map(fetch, urls))
```

Click the virtual annotation to inspect the justification, source evidence, alternatives, assumptions and unanswered questions—including why the worker count is eight.

## Install

Requires Python 3.11+, Git and [uv](https://docs.astral.sh/uv/).

```bash
uv sync
uv run wy --help
# Optional: expose wy on PATH for use in other repositories and VS Code
uv tool install .
```

No backend, telemetry or network request is involved in the default analysis path. Local artifacts live in `.wy/`; add that directory to your own `.gitignore` before sharing a repository. wy does not edit it for you.

## Try the complete workflow

From this checkout, create a new throwaway repository with a representative change and a synthetic Codex transcript. No sample application code is executed, and no application dependencies are required.

```bash
uv run python examples/make_demo.py /tmp/wy-demo
uv run wy review --repo /tmp/wy-demo --session /tmp/wy-demo/.wy/demo-session.jsonl
uv run wy explain worker.py:11 --repo /tmp/wy-demo
uv run wy decisions --repo /tmp/wy-demo --json
uv run wy gaps --repo /tmp/wy-demo
uv run wy ask worker.py:11 'What alternatives exist?' --repo /tmp/wy-demo
```

The demo prints a baseline ID. To isolate the changes after that snapshot, pass `--baseline snapshot-…` to `wy review`. Without the baseline, wy still explains the diff but labels authorship **unknown**.

To see the annotations in VS Code:

```bash
npm install --prefix vscode
npm run compile --prefix vscode
code --extensionDevelopmentPath="$PWD/vscode" /tmp/wy-demo
```

Open `worker.py`. A CodeLens appears above line 11. Click it to open the evidence panel. Enable VS Code's `editor.codeLens` setting if lenses are hidden. The extension can display an existing cache without the CLI on PATH. For its explicit review and follow-up actions, set the **user-level** `wy.executable` setting to this checkout's absolute `.venv/bin/wy` path, or install `wy` on PATH. Open the Git root as the workspace folder.

The extension is a local development extension, not a published Marketplace release. Its **wy: Review Current Changes** command runs offline against HEAD; use the CLI for session selection, baseline selection or model analysis. **wy: Refresh Decision Annotations** reloads a cache manually if filesystem notifications are unavailable.

## Use with Codex

Before an agent begins, capture your working tree:

```bash
wy snapshot --json
# Run your coding agent as usual.
wy sessions
wy review --session <id> --baseline <snapshot-id>
```

`wy sessions` discovers metadata in `$CODEX_HOME/sessions` and `archived_sessions` (default `~/.codex`). It does not read auth files. Supply a rollout path or an exported `codex exec --json` JSONL path directly with `--session` if desired. JSON exec streams may lack the original user request and workspace metadata; wy reports that limitation. Missing history is supported, not fabricated.

A baseline isolates net changes made **since the snapshot**. It cannot prove who authored them if humans or other agents were editing concurrently. A review without a baseline cannot separate agent edits from pre-existing uncommitted work. The chosen transcript is contextual evidence, not an authorship filter.

## Commands

### Open the terminal workspace

Run `wy` from a repository. It opens a persistent terminal workspace with a decision
tree, code panes, a Codex conversation browser, navigation history and an in-app
command bar. `wy --repo PATH` opens another repository; `wy explore` is an alias.
From an uninstalled checkout, use `uv run wy`.

Type `/` while navigating, or press **Ctrl+J**, to focus the command bar. Matching
commands appear as you type. Start with `/help`:

| In-app command | Action |
| --- | --- |
| `/decisions` | Focus the decision tree |
| `/decision 3` | Open a numbered decision |
| `/evidence 2` | Follow the selected decision's second citation |
| `/session` | Browse the saved Codex conversation |
| `/event event-42` | Open a stored event |
| `/find SQLite` | Filter decisions |
| `/code` | Show saved and current source panes |
| `/line 120` | Go to a line in the current file |
| `/ask What alternatives exist?` | Investigate the selected decision offline |
| `/origin` | Inspect review ID, time, session and limitations |
| `/back`, `/forward` | Move through visited decisions and evidence |
| `/review` | Explicitly create a fresh offline review |
| `/review <session-id-or-path>` | Create a review with Codex evidence |
| `/quit` | Exit |

Click or press Enter on a decision, and expand it to follow its citations.
Implementation/configuration findings are grouped separately from examples,
fixtures, tests and documentation. These groups describe file locations; they do
not certify that a detected pattern is a real design decision.

**Tab** changes focus; **Alt+Left / Alt+Right** navigate history; **Ctrl+K** searches
decisions; **Ctrl+F** searches the current code file; **Ctrl+G** goes to a line;
**Ctrl+Q** exits. Source panes are read-only and include line numbers and syntax
highlighting. Code appears when you select a code citation.

The code view preserves the saved excerpt and separately displays the current
file. If the file changed, wy searches for an exact excerpt match: a unique match
helps locate moved text, while multiple matches or changed text remain explicitly
unresolved. Navigation never rewrites the original citation or clears staleness.
Conversation citations open the stored event; previous/next controls and linked
tool call/result navigation expose its context. Nearby events do not establish a
causal chain or private model reasoning.

The Origin tab shows the saved review and selected Codex session. That session is
contextual evidence, not proof of authorship. Later assessments show self-reported
identity separately; their authoring session was not captured by the current import
format. Browsing uses saved data without invoking a model or rereading the live
transcript. Only the explicit `/review` command creates a new review.

The standalone commands remain available for scripts and quick inspection:

```bash
wy decisions              # compact numbered index and review origin
wy explain 1              # rationale, citations and later assessments
wy evidence 1 2           # saved evidence plus current source context
wy session                # linked session and cited events
wy session --event event-42
wy explain 1 --show-code
```

Numbers refer to the current review's order; `wy gaps` preserves those numbers.
Use full decision IDs when retaining a reference. Existing JSON output remains
available with `--json`; `wy evidence` and `wy session` also support it.

All noninteractive commands support `--json`. Repository commands accept `--repo PATH`.

| Command | Purpose |
| --- | --- |
| `wy sessions` | List discovered Codex sessions; `--codex-home PATH` overrides discovery |
| `wy snapshot` | Save a pre-session working-tree baseline |
| `wy review` | Analyze HEAD versus the working tree, including staged and untracked files |
| `wy review --session ID_OR_PATH` | Include observable agent history |
| `wy review --baseline ID` | Compare against a captured pre-session tree |
| `wy review --base REV` | Compare against a different commit |
| `wy review --diff change.patch` | Read a unified diff whose added lines match the current working tree |
| `wy explain worker.py:21` | Inspect a decision at a location; a decision ID also works |
| `wy decisions` | Read cached decisions and check staleness |
| `wy gaps` | Show unexplained choices, open questions and stale findings |
| `wy ask worker.py:21 'Why this approach?'` | Investigate a cached decision |

Choose only one of `--baseline`, `--base` and `--diff`. Imported patches are **never applied**. Cached reads do not invoke a model. Follow-ups about why, alternatives, evidence, conventions and assumptions work offline; arbitrary semantic questions require `--model`.

## What the statuses mean

- **Recorded:** a relevant, explicitly stated assistant justification is quoted from the available transcript. It is not access to private reasoning or proof the justification is correct.
- **Inferred:** repository evidence supports a plausible hypothesis, with assumptions made explicit. It is not a verified account of the agent's intent.
- **Unexplained:** the choice is visible, but the available evidence does not establish its motivation.

Alternatives are options to investigate, not a claim that the original agent considered them. Conflicting patterns remain questions rather than being flattened into a single supposed repository convention.

## Optional model analysis

### Ask the coding conversation to review its own decisions

Use the active Codex conversation to supply the assessment, with no separate model provider:

```bash
wy review --session <session-id>
# Nominate a question that automatic detection missed (also works for unchanged code):
wy focus src/wy/storage.py:27 'Why store artifacts in SQLite?' --evidence src/wy/storage.py:41
wy reflection-request src/wy/storage.py:27 --json
# Ask the conversation to answer the returned request using its response_schema,
# write the JSON response to a local file, then import it:
wy record-reflection /tmp/wy-reflection.json
wy explain src/wy/storage.py:27
```

For wy itself, run these commands from this checkout with `uv run wy`. The request includes
code excerpts, citation IDs and a schema for rationale, alternatives, assumptions, uncertainty,
and a keep/revise/insufficient-context assessment. A response is a **retrospective assessment**;
it never replaces the original explanation or upgrades it to Recorded. Agent/model identity
and original-conversation context are self-reported. Use `separate_review` when the original
coding conversation is unavailable, and `null` when the model name is unknown.

The CLI validates request identity, decision membership, citations and source freshness before
saving. It never starts another model or applies suggested changes. `wy explain --json` returns
the complete reflection history; terminal output summarizes it. Existing editor annotations
continue to show the original explanation. A new `wy review` starts fresh decisions; prior
reviews and reflections remain in the local SQLite artifact history.

### Use a separate configured model

The provider boundary is extensible. The included implementation speaks the [Ollama chat API](https://docs.ollama.com/api/chat), requests a JSON schema, validates the output with Pydantic and rejects fabricated evidence IDs. Start a local Ollama service with a model you have explicitly chosen and installed:

```bash
export WY_MODEL='your-installed-model'
# Defaults to http://127.0.0.1:11434/api/chat
export WY_MODEL_ENDPOINT='http://127.0.0.1:11434/api/chat'
wy review --model
wy ask worker.py:21 'What assumptions should I verify?' --model
```

A remote Ollama-compatible endpoint additionally requires **both** HTTPS and `WY_ALLOW_REMOTE=1`. Optional `WY_PROVIDER_API_KEY` is passed as a bearer token, never stored. These settings are read from the process environment, not repository files. `--model` is required even when the environment is configured. It sends redacted decision excerpts and retrieved evidence, not the whole repository. Review the data before enabling external processing: redaction is best-effort.

Invalid model results leave the offline decision intact and add a warning. Models cannot assign `Recorded`, change code anchors or introduce arbitrary source citations. Citation existence is validated; semantic entailment still requires human review. Token counts returned by the provider are saved per review. No monetary price is guessed.

## Verify

```bash
uv run pytest -q
uv run ruff check src tests benchmarks examples
uv run python benchmarks/run.py
npm test --prefix vscode
```

Tests cover missing history and evidence, unrelated pre-existing edits, competing conventions, malformed model output, secrets, non-invasive review, unsafe paths, stale anchors and editor provider behavior. The small synthetic benchmark is a regression suite, not a claim of real-world accuracy or human-rated usefulness. See [evaluation](docs/evaluation.md).

See [architecture and limitations](docs/architecture.md), [security](docs/security.md), and [Codex format support](docs/codex-formats.md).
