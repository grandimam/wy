<div align="center">

<img src="docs/assets/wy-owl-upright.png" alt="wy's front-facing pixel owl mascot" width="144" height="144">

# wy

### A terminal tool for reviewing AI-generated code.

**Read the reason. Then the change it made.**

wy puts the agent's own explanation above each change it produced, straight from your saved Codex or Claude Code sessions, so you can judge the reasoning before the code.

[![Experimental](https://img.shields.io/badge/Status-Experimental-E7B66D?style=flat-square&labelColor=182335)](#experimental-status)
[![Codex + Claude Code](https://img.shields.io/badge/Works_with-Codex_%2B_Claude_Code-78DCCE?style=flat-square&labelColor=182335)](#how-evidence-is-captured-and-explained)
[![Offline by default](https://img.shields.io/badge/Offline-by_default-78DCCE?style=flat-square&labelColor=182335)](#local-by-default-ai-when-you-ask)
[![MIT License](https://img.shields.io/badge/License-MIT-A9A1FF?style=flat-square&labelColor=182335)](LICENSE)

[Get started](#get-started) · [How it works](#how-evidence-is-captured-and-explained) · [Usage guide](docs/usage.md)

</div>

Your agent adds a cache. **Why was it needed, and what happens when the cached data changes?** wy shows what the agent said just before it made the edit, next to the edit itself, so you can check the stated reason and investigate the tradeoff.

```
 wy  payments · 4 files                                              codex

  ▾ src/ 2              │  src/cache.rs
    ▾ http/ 1           │
      ▸ client.rs +3    │  █1█ I'll keep responses in memory.  gpt-5.4
›   ▸ cache.rs +4 −2    │  ┃ Repeated reads can then reuse a response without
  ▾ tests/ 1            │  ┃ another network call; the upstream API is rate limited.
      cache.rs +6       │  ┃ You asked: "Avoid fetching the same response repeatedly."
    README.md +2 −1     │
                        │  @@ line 10 · fn get()  +2 −1  turn ›
                        │   fn get() {
                        │  -    fetch()
                        │  +    cache.get(key)
                        │  +        .unwrap_or_else(fetch)
                        │   }
                        │
                        │  @@ line 42 · fn refresh()  +1 −0  · no recorded reason
                        │  +    log::debug!("hit");
                        │
 ↑↓ files   Tab read   e explain   ? more
```

*Sample data. The file tree on the left; one reader on the right with each reason placed above the changes it explains. wy uses your terminal's own colours.*

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

1. **Choose a file** with **↑ / ↓** or a click. The reader shows its changes in order, with the agent's reason above each one and the model that wrote it.
2. **Open a turn.** Press **Enter**, move to a change with **↑ / ↓**, and press **Enter** again to read the whole conversation turn that made it. **w** collapses the reasons to one line each.
3. **Press e** to **Enrich** when you want a fuller, cited explanation. Keep browsing while it runs; the file shows **ready** when you can return.

**Tab** moves between the file tree and the reader · **Esc** goes back · **i** asks a follow-up · **?** opens help · **q** quits.

Codex is the default answering agent; type `/agent claude` to switch. Enrichment requires the chosen CLI to be installed and signed in. Reasons, changes and saved evidence work offline.

There are no subcommands: everything happens inside the app. [All shortcuts](docs/usage.md#terminal-workspace) · [Agent explanations](docs/usage.md#agent-explanations)

## How evidence is captured and explained

1. **Read existing session logs.** Work with Codex or Claude Code as usual. When you open wy, it discovers local logs whose recorded working directory belongs to your repository. You do not need to start a wy recorder before coding.
2. **Match each change to the edit that made it.** wy reads the Git diff and the edit records in the logs (Codex patches; Claude Write, Edit and MultiEdit calls) and compares the changed lines exactly. A matching hunk gets the message the agent wrote just before that edit as its reason, plus your request that started the turn. One message that led to several edits appears once. A hunk with no matching edit says **no recorded reason**; wy never invents one.
3. **Explain when you ask.** Press **e** to have your installed agent CLI select relevant code and explain it using bounded, redacted code, diff and conversation excerpts. wy checks that citations refer to supplied evidence and that a **Recorded** reason includes an exact saved assistant quote.

A reason is what the agent wrote, not its hidden reasoning, and a match shows the transcript recorded that edit, not who typed the final text. Changes made through shell commands or formatters leave no edit record, so they show as unexplained.

*Illustrative explanation of a configuration cache:*

> **Recorded reason:** “I cached `load_config` to avoid reading the same configuration file on every request.” `[1]`
>
> **Implementation:** `load_config` now uses `@lru_cache(maxsize=128)` to reuse results for the same arguments. `[2]`
>
> **Check next:** If the configuration file changes while the process is running, should the next call see the new contents?

Opening a citation like `[1]` shows the saved assistant message and surrounding conversation; `[2]` shows the supporting code. The answer is a fresh assessment of that evidence. wy saves it with its sources and flags later code changes when you reopen it.

After committing reviewed changes, press **g** in the workspace to browse commits, or enter `/commit <hash>` to read their saved conversations. wy automatically matches a saved review's base and source snapshot to the commit during lookup. Use `/link <hash> [review-id]` for an explicit association. Drag the divider or use **[ / ]** to resize the file tree; the width is remembered for the repository. See [conversation lookup by commit](docs/usage.md#find-conversations-by-commit) for matching rules and local-storage limits.

Compacted summaries are labeled **Secondary evidence**. Their source links open saved original messages when references can be verified. Otherwise wy explicitly shows **Original turn unavailable** and leaves the original rationale unknown. Summaries and unclassified older captures cannot establish a **Recorded** reason. See [summary provenance](docs/usage.md#summaries-and-original-turns) for resumed and branched sessions.

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
| **Browsing** · reasons, changes, turns, commits | Git diffs and saved conversation excerpts. No model or network request. | A Git repository. |
| **Agent explanation** · **e**, `/reason`, `/why` or `/ask` | Your installed Codex or Claude CLI receives bounded, redacted evidence and returns a fresh assessment. | The selected CLI, sign-in and available account usage. |

wy does not modify application source or execute the code it reviews. Reviews, source snapshots and normalized conversation excerpts stay in local `.wy/` artifacts unless you explicitly request model processing. Private reasoning is excluded. Redaction is best-effort; local artifacts are not encrypted. Add `.wy/` to your repository's `.gitignore` before sharing it.

## Experimental status

**wy is experimental software (0.2.0).** The interface and saved artifact formats are evolving. Use its explanations as a starting point for investigation, and verify important claims against the cited code and conversation.

Rust, JavaScript, TypeScript and JSON use tree-sitter syntax navigation; other text files use line anchors. Retrieval is lexical, and citations need human judgment.

## Development

From the checkout:

```bash
cargo check --locked
cargo test --locked
```

Automated checks verify regressions, not real-world accuracy. See [evaluation](docs/evaluation.md) for its scope.

[Usage guide](docs/usage.md) · [Release setup](docs/releasing.md) · [Architecture and limitations](docs/architecture.md) · [Security boundaries](docs/security.md) · [Codex format support](docs/codex-formats.md)

Licensed under [MIT](LICENSE).
