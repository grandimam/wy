<div align="center">

# wy

### An experimental IDE for understanding AI-generated code.

**AI wrote the code. Know why before you own it.**

Explore the implementation, question the design, and follow every explanation back to its evidence.

[![Experimental](https://img.shields.io/badge/Status-Experimental-E7B66D?style=flat-square&labelColor=182335)](#experimental-status)
[![Codex + Claude Code](https://img.shields.io/badge/Works_with-Codex_%2B_Claude_Code-78DCCE?style=flat-square&labelColor=182335)](#from-agent-output-to-code-you-understand)
[![Offline by default](https://img.shields.io/badge/Offline-by_default-78DCCE?style=flat-square&labelColor=182335)](#local-by-default-ai-when-you-ask)
[![MIT License](https://img.shields.io/badge/License-MIT-A9A1FF?style=flat-square&labelColor=182335)](LICENSE)

[Get started](#get-started) · [Explore the IDE](#an-ide-built-around-understanding) · [Command guide](docs/usage.md)

</div>



A diff tells you **what changed**. wy helps you investigate **why that approach was chosen**, **what assumptions remain**, and **what you should check before shipping**.

**wy is a terminal-based IDE for understanding AI-generated code.** It brings your Git changes, source files, design questions, and project-matched Codex or Claude Code conversations into one place. Explore a change from the overall behavior down to a single class or function, then inspect the evidence behind the answer.

The IDE is read-only: you use it to understand and evaluate the implementation. Offline review works without an account; deeper explanations use your installed Codex or Claude CLI when you ask.

## From agent output to code you understand

| You want to know… | wy gives you… |
| --- | --- |
| **What problem does this change solve?** | An opt-in agent explanation of the problem, before/after behavior, tradeoffs and checks. |
| **Why this implementation?** | Detected choices with recorded rationale, evidence-backed hypotheses or an explicit “unexplained” status. |
| **Where did that explanation come from?** | Navigable citations into code, saved conversation events and linked tool calls/results. |
| **What still needs checking?** | Assumptions, alternatives, unresolved questions and stale evidence. |
| **Does the evidence still match the code?** | Saved excerpts beside current source, with changed or ambiguous locations flagged. |

## An IDE built around understanding

**Explore a file → understand the implementation → question the design → inspect the evidence.**

| Part of the IDE | What you can do |
| --- | --- |
| **Changed-file explorer** | Browse changes by folder, expand a file into classes and functions, and choose exactly what to investigate. Refresh the file list offline. |
| **Explanation workspace** | Read how the implementation works, why the choices fit, what alternatives exist, and what to check. Keep the code and its reasoning in the same workspace. |
| **Design investigation** | Select a class, function or line and ask **Why this design?** No automatically detected decision is required. |
| **Evidence navigation** | Click a numbered reference to inspect the captured code, diff or conversation. Return with **← Explanation** or **Escape**. |
| **Follow-up questions** | Ask “Would a simpler function work?” or “What happens if this fails?” while keeping the selected target in scope. |
| **Code and conversation views** | Compare saved citations with current source, navigate file outlines, and inspect the conversation surrounding a recorded statement. |

**Settings** holds your Codex/Claude choice, history sources, detected choices and review details. **Files** toggles the explorer on a narrow terminal; **Help** opens a short guide. Saved explanations and evidence can be browsed without another model call.


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
wy                        # Open the IDE
```

Click **Explain changes** to understand the recent work, or choose a file, class or function in the explorer to investigate one part. Select Codex or Claude in **Settings**. Generating explanations requires the chosen CLI to be installed and signed in, and may consume your account's usage.

For the offline review, open **Settings → Detected choices** to inspect the detected decisions, rationale and citations.

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

To understand a particular design choice after the AI has coded:

```bash
wy why src/example.rs:MyClass
wy why src/example.rs:MyClass.run --question 'Would a simpler function work?'
```

In `wy`, expand a changed file, select a class or function, and choose **Why this
design?**. The answer distinguishes recorded justifications, inferred benefits,
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

Inside the IDE, **Ctrl+B** toggles files, **Ctrl+J** opens the command bar, and **F1** opens help. **Tab** moves focus, **Alt+Left / Alt+Right** navigate history, and **Ctrl+Q** exits.

In-app `/ask` calls the selected agent CLI. The shell command `wy ask TARGET 'QUESTION'` works offline unless you add `--model`.

## Experimental status

**wy is experimental software (0.2.0).** The interface, commands and saved artifact formats are evolving. Use its explanations as a starting point for investigation, and verify important claims against the cited code and conversation.

Offline detection is deliberately selective and capped at twelve decisions per review. It covers patterns such as concurrency, caching, dependency versions, operational limits, broad exception handling and retries; it does not discover every architectural decision. Rust, JavaScript, TypeScript and JSON use tree-sitter syntax navigation; other text files use line anchors. Retrieval is lexical, and citations need human judgment.

## Development

From the checkout:

```bash
cargo check --locked
cargo test --locked
```

Automated checks verify regressions, not real-world accuracy. See [evaluation](docs/evaluation.md) for its scope.

[Command guide](docs/usage.md) · [Architecture and limitations](docs/architecture.md) · [Security boundaries](docs/security.md) · [Codex format support](docs/codex-formats.md)

Licensed under [MIT](LICENSE).
