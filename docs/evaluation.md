# Evaluation

Run `uv run python benchmarks/run.py` for the ten-case synthetic offline regression suite. Cases model batch-download concurrency, shared caching, database indexing, failure suppression, repository abstractions, operational limits, dependency changes, an ordinary bug fix, hostile source comments and competing concurrency conventions.

The suite checks decision identification against explicit expected file/category labels, provenance labels, exact source excerpt/range correspondence, and supported-vs-unexplained status. It reports token use as zero because the offline engine makes no model requests. `docs/benchmark-results.json` records this checkout's result.

These are synthetic, implementation-aware regression fixtures—not a held-out corpus of real agent runs. Perfect results here do not establish production precision or recall. Citation accuracy checks where a quote came from, not whether that quote entails every claim. The unsupported-explanation metric compares fixture labels; it is not a general hallucination detector.

Human evaluation should use at least two engineers, blinded to agent/model identity, with access to the original change and observable transcript. Score each annotation independently:

| Dimension | Measurement |
| --- | --- |
| Decision precision | Fraction that identify a consequential choice rather than restating behavior |
| Annotation usefulness | 1–5: did it help the reviewer assess or modify the change? |
| Evidence citation accuracy | Does the cited snapshot and range contain the supporting fact? |
| Unsupported explanation rate | Fraction containing a factual claim unsupported by the cited evidence |
| Explanation relevance | 1–5: does the explanation answer why this implementation was chosen? |
| Uncertainty honesty | Are hypotheses and missing evidence clearly distinguished from recorded statements? |
| Staleness | Lenses disappear after target edits, evidence edits, deletions and dirty buffers |
| Cost | Provider-reported input/output tokens per review; actual billed cost from provider accounting |

Automated Python tests exercise baselines, unrelated changes, absent history/evidence, conflicting conventions, malformed model JSON, fabricated citations, secret redaction, source immutability and stale decisions. TypeScript tests exercise path validation, HTML escaping, hash matching, CodeLens registration, rendered panel content and dirty/stale source suppression through an API double. A real-host smoke runner is also available in `vscode/test/host.cjs`.

Optional model adapters are tested with synthetic transport responses. The project does not claim quality or cost results for a real model until it has been explicitly configured and evaluated. No source was sent to an external model to produce the checked-in benchmark results.

## Verified in this checkout

The Python suite passed 43 tests; the extension compiled and passed six tests. A real VS Code development host also passed the demo smoke test: the Recorded lens appeared at `worker.py:11`, the evidence panel opened, and an unsaved edit removed the lens. The demo source was restored afterward.

To reproduce the real-host check after creating and reviewing the demo:

```bash
code --extensionDevelopmentPath="$PWD/vscode" \
  --extensionTestsPath="$PWD/vscode/test/host.cjs" \
  --user-data-dir=/tmp/wy-vscode-test-user \
  --extensions-dir=/tmp/wy-vscode-test-extensions \
  --disable-extensions --disable-workspace-trust \
  --skip-welcome --skip-release-notes --wait /tmp/wy-demo
```

The isolated test window closes on completion. Look for `WY_HOST_SMOKE_PASS` in its renderer log under the temporary user-data directory. The test changes only the throwaway demo's unsaved buffer and reverts it.
