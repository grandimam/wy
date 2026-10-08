import importlib.util
from pathlib import Path


def test_synthetic_regression_benchmark():
    path = Path(__file__).parents[1] / "benchmarks" / "run.py"
    spec = importlib.util.spec_from_file_location("benchmark", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    result = module.evaluate()
    assert result["decision_precision"] == 1
    assert result["decision_recall"] == 1
    assert result["evidence_citation_accuracy"] == 1
    assert result["unsupported_explanation_rate"] == 0
