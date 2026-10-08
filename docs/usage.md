# Using wy

[← Back to the README](../README.md)

Detailed workflows for the terminal workspace, project history, evidence and optional model analysis.

## Use with Codex, Claude Code, or both

Before an agent begins, capture your working tree:

```bash
wy snapshot --json
# Run your coding agent as usual.
wy sessions
wy review --session <id> --baseline <snapshot-id>
```

`wy sessions` lists only sessions whose recorded working directory resolves to the
current Git repository. A subdirectory in the same repository matches; another
repository, a nested repository, another worktree or missing workspace metadata
does not. Explicitly selected unrelated/unverified transcripts are rejected too.

History sources:

- Codex: `$CODEX_HOME/sessions` and `archived_sessions` (default `~/.codex`).
- Claude Code: `$CLAUDE_CONFIG_DIR/projects/*/*.jsonl` (default `~/.claude`).
- Equivalent history layouts under this repository's `.codex` and `.claude` are
  also discovered. Settings/instruction files are not conversation transcripts.

`wy review` defaults to both agents' matched project history. Choose
`wy review --source codex`, `--source claude`, `--source both`, or `--source none`.
Repeat `--session` to select specific IDs, `agent:id` identifiers, or JSONL paths.
When no matching sessions exist, the review uses repository evidence only.
Automatically selected history is capped at 20 recent sessions and 40 MB total
(20 MB per transcript), with omissions reported. Both providers are interleaved
before applying the budget. Browsing and `/review` do not invoke a model; explanations are requested separately.

Claude ingestion retains observable text, tool calls and tool results, omitting
thinking/redacted-thinking blocks, system and progress records. Tool results keep
their call IDs. Session citations preserve the agent, session ID and original
transcript location; histories are not merged under a synthetic author. Each new
review references saved session snapshots, so a subsequent review cannot silently
replace earlier evidence. Exported Codex exec streams without workspace metadata
cannot be used until that association can be verified.

A baseline isolates net changes made **since the snapshot**. It cannot prove who authored them if humans or other agents were editing concurrently. A review without a baseline cannot separate agent edits from pre-existing uncommitted work. The chosen transcript is contextual evidence, not an authorship filter.

## Commands

### Open the terminal workspace

Run `wy` from a repository. It opens **Understand**, where you choose your installed
**Codex CLI** or **Claude CLI** and select **Explain changes**. No local model is
needed. The CLI uses your existing sign-in and account usage.

Explanations connect the request, before/after behavior, mechanisms, tradeoffs and
checks to numbered evidence. Select a citation to inspect the exact diff, source
excerpt or session event supplied to the model. Observations, new assessments and
proposed checks are labeled separately. This is a fresh assessment, not recovered
private reasoning or proof of who wrote a change. Ask follow-up questions in the
input below the explanation. Each question receives a fresh evidence packet.

The persistent terminal workspace also includes a decision
tree, code panes, a Codex/Claude conversation browser, navigation history and an in-app
command bar. `wy --repo PATH` opens another repository; `wy explore` is an alias.
From an uninstalled checkout, use `uv run wy`.

Type `/` while navigating, or press **Ctrl+J**, to focus the command bar. Matching
commands appear as you type. Start with `/help`:

| In-app command | Action |
| --- | --- |
| `/decisions` | Focus the decision tree |
| `/decision 3` | Open a numbered decision |
| `/evidence 2` | Follow the selected decision's second citation |
| `/session` | Browse reviewed project sessions; select a session in the conversation pane |
| `/sessions` | List discovered Codex/Claude sessions for this repository |
| `/event event-42` | Open a stored event |
| `/find SQLite` | Filter decisions |
| `/code` | Show saved and current source panes |
| `/line 120` | Go to a line in the current file |
| `/reason codex` or `/reason claude` | Explain current changes through that installed CLI |
| `/reason codex src/example.py` | Explain one file |
| `/ask What alternatives exist?` | Ask the selected CLI about the selected decision’s file |
| `/cancel` | Stop the running explanation |
| `/origin` | Inspect review ID, time, session and limitations |
| `/back`, `/forward` | Move through visited decisions and evidence |
| `/review` | Create a review using the History source selector (both agents by default) |
| `/review codex`, `/review claude`, `/review both` | Review with only that project history |
| `/review none` | Review repository evidence without agent history |
| `/review <session-id-or-path> …` | Review with specific project sessions |
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
file. It starts at a bounded cited region; **Full file** expands it. Python, Markdown
and JSON files offer a structural outline. Search and go-to use original file line
numbers even in the focused view. If the file changed, wy searches for an exact excerpt match: a unique match
helps locate moved text, while multiple matches or changed text remain explicitly
unresolved. Navigation never rewrites the original citation or clears staleness.
Conversation citations open the stored event; previous/next controls and linked
tool call/result navigation expose its context. Nearby events do not establish a
causal chain or private model reasoning.

The Origin tab shows the saved review and every included agent/session. These
sessions are contextual evidence, not proof of authorship. Later assessments show self-reported
identity separately; their authoring session was not captured by the current import
format. Browsing uses saved data without invoking a model or rereading the live
transcript. An explicit explanation request refreshes stale reviews before collecting its evidence.
Generated explanations are saved separately under `.wy` and rejected if the code
changes while generation is running.

The standalone commands remain available for scripts and quick inspection:

```bash
wy reason --agent codex   # explain actual changes; claude is also supported
wy reason --agent claude --file src/example.py --question "Why this approach?"
wy decisions              # compact numbered index and review origin
wy explain 1              # rationale, citations and later assessments
wy evidence 1 2           # saved evidence plus current source context
wy session                # linked session and cited events
wy session --id codex:<session-id> --event event-42
wy explain 1 --show-code
```

Numbers refer to the current review's order; `wy gaps` preserves those numbers.
Use full decision IDs when retaining a reference. Existing JSON output remains
available with `--json`; `wy evidence` and `wy session` also support it.

Data-producing commands support `--json`. Repository commands accept `--repo PATH`.

| Command | Purpose |
| --- | --- |
| `wy sessions` | List project-matched sessions; filter with `--source codex`, `claude`, or `both` |
| `wy snapshot` | Save a pre-session working-tree baseline |
| `wy review` | Analyze HEAD versus the working tree, including staged and untracked files |
| `wy review --source codex` | Analyze using only this repository’s Codex history (`claude`, `both`, `none` also supported) |
| `wy review --session ID_OR_PATH` | Select project-scoped agent history; repeat for multiple sessions |
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

### Explain changes with an installed agent CLI

Install and sign in to Codex or Claude Code, then run:

```bash
wy reason --agent codex
wy reason --agent claude --file worker.py
wy reason --agent codex --question 'What behavior changed, and what should I test?'
```

In the workspace, choose **Codex CLI** or **Claude CLI** in the **Understand** tab
and click **Explain changes**, or use `/reason codex`. Select a citation to inspect
the supplied diff, code excerpt or stored conversation event in the **Evidence** tab.
Use `/cancel` to stop an in-progress request.

This is an explicit model call through your existing CLI sign-in and may consume
your account's usage. wy supplies bounded, redacted diff/source/history excerpts;
the agent runs from a temporary directory with tools disabled. The result explains
the problem, before/after behavior, tradeoffs, proposed checks and unknowns.
Claims distinguish observations, assessments, proposals and unknowns. Citation IDs
and source freshness are validated before saving; semantic correctness still needs
human review. This is a new assessment, not recovered original intent.

**The two ask commands differ:** in-app `/ask` calls the selected agent CLI;
standalone `wy ask TARGET 'QUESTION'` uses cached evidence offline unless `--model`
is explicitly supplied.


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
the complete reflection history; terminal output summarizes it. A new `wy review` starts fresh decisions; prior
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

## Screenshots

These are actual terminal captures of the bundled threaded-worker demo, using a
synthetic Codex transcript and offline review. No model-generated assessment is
shown.

![Decision workspace with recorded rationale, alternatives and an open question](assets/workspace.svg)

![Code evidence with the saved citation beside the current source file](assets/evidence.svg)
