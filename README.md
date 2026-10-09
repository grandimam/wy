<div align="center">

<img src="docs/assets/wy-owl-upright.png" alt="wy's front-facing pixel owl mascot" width="144" height="144">

# wy

### See why your AI agent made each change.

[![Experimental](https://img.shields.io/badge/Status-Experimental-E7B66D?style=flat-square&labelColor=182335)](#status)
[![Codex + Claude Code](https://img.shields.io/badge/Works_with-Codex_%2B_Claude_Code-78DCCE?style=flat-square&labelColor=182335)](#how-it-works)
[![Offline by default](https://img.shields.io/badge/Offline-by_default-78DCCE?style=flat-square&labelColor=182335)](#what-is-true-and-what-is-not)
[![MIT License](https://img.shields.io/badge/License-MIT-A9A1FF?style=flat-square&labelColor=182335)](LICENSE)

</div>

Your agent changed 40 files. You can read the diff, but the reason for each change is somewhere in a transcript you will never scroll back through.

wy reads your saved Codex and Claude Code sessions, matches each diff hunk to the edit that produced it, and shows what the agent said right before it made that edit. Above the code. Nothing invented.

![wy terminal UI: the file tree on the left; on the right, the agent's numbered reason above the diff hunks it produced](docs/assets/terminal-preview.png)

## Get started

Needs Rust 1.88+, Git and a C toolchain.

```bash
cargo install --git https://github.com/grandimam/wy --locked wy-code
cd /path/to/your/repo
wy
```

Then:

- **Pick a file.** Its changes appear in order, each under the reason the agent gave and the model that wrote it.
- **Press Enter on a change** to read the whole conversation turn that made it.
- **Press e** for a fuller explanation with cited sources, written by your Codex or Claude CLI.

**Tab** switches between the tree and the reader · **w** collapses reasons to one line · **?** shows every key.

## How it works

1. wy finds local Codex and Claude Code logs whose working directory is your repository. No recorder to start first.
2. It reads the Git diff and the edit records in those logs (Codex patches; Claude Write, Edit and MultiEdit), and compares the changed lines exactly.
3. A hunk that matches an edit gets the message the agent wrote just before it, plus your request that started the turn. One message that led to several hunks appears once.

## What is true, and what is not

- A reason is what the agent **wrote**, not its hidden thinking.
- A match means the transcript **recorded** that edit, not who typed the final text.
- Shell commands, formatters and hand edits leave no edit record, so those hunks say **no recorded reason**. wy never fills the gap.
- Everything above works offline. **e** is the only step that calls a model, through your own signed-in CLI.

## Status

**Experimental (0.2.0).** The interface and saved formats are still changing. Treat explanations as a starting point and check them against the cited code and conversation.

## Learn more

[Usage guide](docs/usage.md) · [Commit lookup](docs/usage.md#find-conversations-by-commit) · [Summaries and original turns](docs/usage.md#summaries-and-original-turns) · [Architecture](docs/architecture.md) · [Security](docs/security.md) · [Codex format support](docs/codex-formats.md) · [Releasing](docs/releasing.md)

Development: `cargo test --locked`. See [evaluation](docs/evaluation.md) for what the tests do and do not cover.

Licensed under [MIT](LICENSE).
