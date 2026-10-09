<div align="center">

# wy

### A terminal tool for reviewing AI-generated code.

**Browse changes. Ask about design choices. Check the evidence.**

Review Git diffs alongside saved Codex or Claude Code conversations, with optional agent explanations.

[![Experimental](https://img.shields.io/badge/Status-Experimental-E7B66D?style=flat-square&labelColor=182335)](#experimental-status)
[![Codex + Claude Code](https://img.shields.io/badge/Works_with-Codex_%2B_Claude_Code-78DCCE?style=flat-square&labelColor=182335)](#what-you-can-investigate)
[![Offline by default](https://img.shields.io/badge/Offline-by_default-78DCCE?style=flat-square&labelColor=182335)](#local-by-default-ai-when-you-ask)
[![MIT License](https://img.shields.io/badge/License-MIT-A9A1FF?style=flat-square&labelColor=182335)](LICENSE)

[Get started](#get-started) · [Terminal review](#review-changes-in-the-terminal) · [Command guide](docs/usage.md)

</div>

A diff shows what changed. wy adds related conversation history and tools to investigate the implementation, possible reasons for a choice, and questions to check during review.

Use it to browse changed files and symbols, inspect diffs, and ask your installed Codex or Claude CLI for a cited explanation. Explanations distinguish recorded justifications from inferred reasons and gaps in the available evidence.

wy does not edit your source files. Offline review and evidence browsing work without a model account; generating an agent explanation requires an explicit request.

## What you can investigate

| Review question | What wy provides |
| --- | --- |
| **What problem does this change solve?** | An opt-in agent explanation of the problem, before/after behavior, tradeoffs and checks. |
| **Why this implementation?** | Detected choices with recorded rationale, evidence-backed hypotheses or an explicit “unexplained” status. |
| **Where did that explanation come from?** | Navigable citations into code, saved conversation events and linked tool calls/results. |
| **What still needs checking?** | Assumptions, alternatives, unresolved questions and stale evidence. |
| **Does the evidence still match the code?** | Saved excerpts beside current source, with changed or ambiguous locations flagged. |

## Review changes in the terminal

**Select a diff → Why this change? → Read the supporting conversation.**

The workspace starts with changed files on the left and their diff on the right. Expand a file to navigate its changed functions. Press **w** or click **Why this change?** to connect the request, the agent's stated justification, and the resulting code.

The answer leads with a recorded reason when the captured conversation contains one. Otherwise, it says what is missing and labels code-based explanations as inferences. Numbered references open the captured statements or code. Press **i** for a follow-up, or **d** to return to the diff; reopening the answer does not make another request.

Use `/agent codex` or `/agent claude` to choose an agent, and `/source both|codex|claude|none` to choose history. **?** opens help and additional commands.

## Get started

```bash
git clone https://github.com/grandimam/wy.git
cd wy
cargo install --path . --locked
```

Then open a repository with changes you want to understand:

```bash
cd /path/to/your/repo
wy review                 # Review the diff and matching project history, offline
wy                        # Open the terminal review interface
```

Choose a changed file or function, then press **w** for **Why this change?**. Use `/agent codex` or `/agent claude`. Generating explanations requires the chosen CLI to be installed and signed in, and may consume your account's usage.

For offline detected decisions, rationale and citations, use `wy decisions` and `wy explain` in your shell.

**No agent history?** Review still works using repository evidence. **No model account?** Offline review and evidence browsing still work.

Local reviews are stored in `.wy/`. Add `.wy/` to your repository's `.gitignore` before sharing it; wy does not edit the ignore file for you.

## Fit it into your agent workflow

Before the agent starts, capture the current working tree:

```bash
wy snapshot --json
# Run Codex, Claude Code, or both as usual.
wy review --baseline <snapshot-id>
wy
```

Without a baseline, wy compares **HEAD to the current working tree**, including staged and non-ignored untracked files. A snapshot separates new changes from pre-existing edits; concurrent human or agent edits can still be included.

Project history is discovered automatically for both agents. Narrow it when needed:

```bash
wy sessions                         # List sessions belonging to this repository
wy review --source claude           # Use only matching Claude Code history
wy review --source none             # Use repository evidence alone
wy review --session codex:<id>       # Select a session; repeat for multiple sessions
```

For a fresh, cited explanation through an installed agent CLI:

```bash
wy reason --agent codex
wy reason --agent claude --file worker.rs
wy reason --agent codex --question 'What changed, and what should I test?'
```

To investigate a particular design choice:

```bash
wy why src/example.rs:MyClass
wy why src/example.rs:MyClass.run --question 'Would a simpler function work?'
```

In `wy`, select a changed file or function and press **w** to investigate its design. The answer distinguishes recorded justifications, inferred benefits,
and missing reasons, with citations you can follow back to code and conversation.
No automatically detected decision is required. Follow-ups keep the selected target.

## Evidence you can question

wy keeps **what was stated**, **what is inferred**, and **what is unknown** distinct.

| Status | Meaning |
| --- | --- |
| **Recorded** | A relevant, explicit justification was found in the agent's observable transcript. |
| **Inferred** | Repository evidence supports a hypothesis; assumptions remain visible. |
| **Unexplained** | The choice is visible, but its motivation is not established by the available evidence. |

A recorded statement can still be wrong. A linked session does not prove authorship. A new model assessment stays separate from the original rationale and never upgrades it to **Recorded**.

## Local by default. AI when you ask.

| Mode | What runs | What you need |
| --- | --- | --- |
| **Offline review** · `wy review`, `wy explain`, `wy ask` | Local detection, evidence retrieval and cached investigation. No model or network request. | A Git repository. |
| **Agent explanation** · `wy reason`, `wy why`, in-app `/reason`, `/why` or `/ask` | Your installed Codex or Claude CLI receives bounded, redacted evidence and returns a fresh assessment. | The selected CLI, sign-in and available account usage. |
| **Optional model enrichment** · `--model` | A configured Ollama-compatible endpoint enriches detected decisions or answers follow-ups. | An explicitly configured model and endpoint. |

wy does not modify application source or execute the code it reviews. Reviews, source snapshots and normalized conversation excerpts stay in local `.wy/` artifacts unless you explicitly request model processing. Redaction is best-effort; local artifacts are not encrypted.

See the [model setup and reflection workflow](docs/usage.md#optional-model-analysis) for Ollama configuration and assessments supplied by an existing coding conversation.

## A few commands worth remembering

| In your shell | Purpose |
| --- | --- |
| `wy why worker.rs:batch` | Investigate the design of a specific function through your agent CLI. |
| `wy reasoning-evidence 1 --id <explanation-id>` | Inspect the exact saved source behind an explanation, offline. |
| `wy decisions` | List detected decisions and review origin. |
| `wy explain 1` | Inspect a decision's rationale, citations and later assessments. |
| `wy evidence 1 2` | Open the second citation for the first decision. |
| `wy gaps` | Find unanswered questions, unexplained choices and stale findings. |
| `wy decisions --json` | Get structured output for scripts. |

In the terminal interface, **Tab** switches between files and the reader, **Space** expands a file's symbols, **f** filters paths, and **m** marks a file reviewed. **Enter** opens the highlighted diff, **w** opens **Why this change?**, **d** returns to the diff, **i** asks a follow-up, and **?** opens help. Mouse navigation and compact terminals are supported.

In-app `/ask` calls the selected agent CLI. The shell command `wy ask TARGET 'QUESTION'` works offline unless you add `--model`.

## Experimental status

**wy is experimental software (0.2.0).** The interface, commands and saved artifact formats are evolving. Use its explanations as a starting point for investigation, and verify important claims against the cited code and conversation.

Offline detection is deliberately selective and capped at twelve decisions per review. It covers patterns for caching, dependency versions, operational limits, database schemas and retries; it does not discover every architectural decision. Rust, JavaScript, TypeScript and JSON use tree-sitter syntax navigation; other text files use line anchors. Retrieval is lexical, and citations need human judgment.

## Development

From the checkout:

```bash
cargo check --locked
cargo test --locked
```

Automated checks verify regressions, not real-world accuracy. See [evaluation](docs/evaluation.md) for its scope.

[Command guide](docs/usage.md) · [Architecture and limitations](docs/architecture.md) · [Security boundaries](docs/security.md) · [Codex format support](docs/codex-formats.md)

Licensed under [MIT](LICENSE).
