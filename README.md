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

[Get started](#get-started) · [How it works](#how-evidence-is-captured-and-explained) · [Command guide](docs/usage.md)

</div>

Your agent adds a cache. **Why was it needed, and what happens when the cached data changes?** wy brings the code and relevant conversation into one review so you can check the stated reason and investigate the tradeoff.

## Get started

Install directly from GitHub with **Rust 1.88+, Git and a C toolchain**:

```bash
cargo install --git https://github.com/grandimam/wy --locked wy-code
```

Open a repository you have been working on:

```bash
cd /path/to/your/repo
wy
```

wy loads your changes and matching agent history automatically.

1. **Choose a file** with **↑ / ↓** or a click to see its changes and available agent notes.
2. **Press e** to **Enrich explanation** when you want more context. Keep browsing while it runs; the file shows **ready** when you can return.
3. **Press s**, choose a source with **↑ / ↓**, and press **Enter** to inspect the evidence. You can also click a source directly.

**Tab** switches panes · **Esc** goes back · **i** asks a follow-up · **?** opens help · **q** quits.

Codex is the default answering agent; type `/agent claude` to switch. Enrichment requires the chosen CLI to be installed and signed in. Changes, agent notes and saved evidence work offline.

[All shortcuts](docs/usage.md#terminal-workspace) · [Review an agent session](docs/usage.md#review-changes) · [Shell commands](docs/usage.md#shell-command-reference)

## How evidence is captured and explained

1. **Read existing session logs.** Work with Codex or Claude Code as usual. When you open wy, it discovers local logs whose recorded working directory belongs to your repository. You do not need to start a wy recorder before coding.
2. **Keep code and context together.** wy reads the Git diff and source files, and imports visible messages, tool calls and results. Supported edit records preserve session code even after it has been committed. Excerpts retain their file, session and event references in local `.wy/` artifacts.
3. **Explain when you ask.** Press **w** to have your installed agent CLI select relevant code and explain it using bounded, redacted code, diff and conversation excerpts. wy checks that citations refer to supplied evidence and that a **Recorded** reason includes an exact saved assistant quote.

*Illustrative explanation of a configuration cache:*

> **Recorded reason:** “I cached `load_config` to avoid reading the same configuration file on every request.” `[1]`
>
> **Implementation:** `load_config` now uses `@lru_cache(maxsize=128)` to reuse results for the same arguments. `[2]`
>
> **Check next:** If the configuration file changes while the process is running, should the next call see the new contents?

Opening a citation like `[1]` shows the saved assistant message and surrounding conversation; `[2]` shows the supporting code. The answer is a fresh assessment of that evidence. wy saves it with its sources and flags later code changes when you reopen it.

See [project history](docs/usage.md#project-history) for log locations and capture limits.

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

wy does not modify application source or execute the code it reviews. Reviews, source snapshots and normalized conversation excerpts stay in local `.wy/` artifacts unless you explicitly request model processing. Private reasoning is excluded. Redaction is best-effort; local artifacts are not encrypted. Add `.wy/` to your repository's `.gitignore` before sharing it.

See the [model setup and reflection workflow](docs/usage.md#optional-model-analysis) for Ollama configuration and assessments supplied by an existing coding conversation.

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

[Command guide](docs/usage.md) · [Release setup](docs/releasing.md) · [Architecture and limitations](docs/architecture.md) · [Security boundaries](docs/security.md) · [Codex format support](docs/codex-formats.md)

Licensed under [MIT](LICENSE).
