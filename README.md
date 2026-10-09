<div align="center">

<img src="docs/assets/wy-owl-upright.png" alt="wy's front-facing pixel owl mascot" width="144" height="144">

# wy

### A terminal tool for reviewing AI-generated code.

**See the change. Read the reason. Open the evidence.**

Connect Git diffs to saved Codex or Claude Code conversations, and ask for an explanation with sources you can inspect.

[![Experimental](https://img.shields.io/badge/Status-Experimental-E7B66D?style=flat-square&labelColor=182335)](#experimental-status)
[![Codex + Claude Code](https://img.shields.io/badge/Works_with-Codex_%2B_Claude_Code-78DCCE?style=flat-square&labelColor=182335)](#how-evidence-is-captured-and-explained)
[![Offline by default](https://img.shields.io/badge/Offline-by_default-78DCCE?style=flat-square&labelColor=182335)](#local-by-default-ai-when-you-ask)
[![MIT License](https://img.shields.io/badge/License-MIT-A9A1FF?style=flat-square&labelColor=182335)](LICENSE)

[Get started](#get-started) · [How it works](#how-evidence-is-captured-and-explained) · [Terminal review](#review-changes-in-the-terminal) · [Command guide](docs/usage.md)

</div>

Your agent adds a cache. **Why was it needed, and what happens when the cached data changes?** wy brings the code and relevant conversation into one review so you can check the stated reason and investigate the tradeoff.

Run `wy` in your repository, select a file or function, and press **w** for **Why this change?**

*Illustrative explanation of a configuration cache:*

> **Recorded reason:** “I cached `load_config` to avoid reading the same configuration file on every request.” `[1]`
>
> **Implementation:** `load_config` now uses `@lru_cache(maxsize=128)` to reuse results for the same arguments. `[2]`
>
> **Check next:** If the configuration file changes while the process is running, should the next call see the new contents?

In the terminal, opening a citation like `[1]` shows the saved assistant message and surrounding conversation; `[2]` shows the supporting code. If the agent never stated a reason, wy labels its explanation **Inferred** or the motivation **Unexplained**.

## How evidence is captured and explained

1. **Work with your agent as usual.** Codex and Claude Code save local session logs. wy reads those existing logs when you open `wy` or run `wy review`; no wy recording process needs to be running during the coding session. It discovers sessions whose recorded working directory belongs to your repository.
2. **Capture the code and its context.** wy reads the Git diff and source files, then imports visible messages, tool calls and results from matching sessions. Supported edit records preserve code from the session, including edits already committed. Saved excerpts retain file, session and event references in local `.wy/` artifacts so you can inspect their origin later.
3. **Ask for an explanation.** Press **w** to have your installed Codex or Claude CLI select relevant code and explain it using bounded, redacted excerpts of the code, diff and conversation. The answer connects the problem, implementation, tradeoffs and suggested checks to citations. wy checks that citations refer to supplied evidence and that a **Recorded** reason includes an exact saved assistant quote.

Evidence browsing works offline. Generating an explanation makes new model calls through your signed-in agent CLI. The explanation is a fresh assessment of the available evidence; private reasoning is excluded. wy saves the answer with its evidence and flags later source changes when you reopen it.

See [project history](docs/usage.md#project-history) for log locations and capture limits.

## Review changes in the terminal

**Select a diff → Why this change? → Read the supporting conversation.**

The workspace starts with changed files and recent session code on the left. Expand a file to navigate its changed functions, or press **c** to inspect its recorded session edit—even after it has been committed. Press **w** or click **Why this change?** to open an explanation beside the code.

The answer reads as prose, with quoted reasons when the conversation contains them and labeled inferences when it does not. Click a source, or press **s** and use Up/Down and Enter, to inspect captured statements or code. Press **i** for a follow-up. Generating an answer makes new model calls for context selection and explanation; reopening a loaded answer makes no new request.

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

## Why Rust instead of Python?

wy started in Python. The [Rust migration](https://github.com/grandimam/wy/commit/4ed78be3c1cb2b56bebb0d90bfdcd22306842071) explicitly cites performance glitches as the reason for the port. The priority is a responsive local review workflow: opening diffs, navigating source and inspecting conversation evidence.

The current Rust implementation supports that workflow in three ways:

- **Compiled local processing.** Transcript parsing, diffing and source navigation run in the compiled application. This targets the local work you wait on while reviewing; generating an explanation still depends on the agent CLI and model response time.
- **Explicit control of the terminal.** Ratatui and Crossterm handle rendering and input. Agent requests run on a background thread, with progress and cancellation, so you can keep navigating while an answer is generated. That responsiveness comes from the application's design as well as its language.
- **One installed executable.** `cargo install` builds `wy`; running it requires no Python interpreter or virtual environment. Git remains required, and agent explanations need the selected Codex or Claude CLI.

The tradeoff is compilation time and Rust's ownership constraints when developing the tool. Improving the Python implementation was also a possible approach. The migration records a performance motivation, but this repository does not include a Python-versus-Rust timing comparison that establishes the size of any speedup.

## Development

From the checkout:

```bash
cargo check --locked
cargo test --locked
```

Automated checks verify regressions, not real-world accuracy. See [evaluation](docs/evaluation.md) for its scope.

[Command guide](docs/usage.md) · [Architecture and limitations](docs/architecture.md) · [Security boundaries](docs/security.md) · [Codex format support](docs/codex-formats.md)

Licensed under [MIT](LICENSE).
