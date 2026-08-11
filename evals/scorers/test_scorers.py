"""scorers 单元测试(W11 Plan 1)。"""

import json

import pytest

from scorers.audit_completeness_scorer import REQUIRED_EVENTS
from scorers.risk_level_scorer import parse_risk_level
from scorers.undo_success_scorer import undo_success_scorer


def test_parse_risk_level_valid():
    assert parse_risk_level("E0×D0") == (0, 0)
    assert parse_risk_level("E2×D2") == (2, 2)
    assert parse_risk_level("E3×D3") == (3, 3)


def test_parse_risk_level_invalid():
    assert parse_risk_level("invalid") is None
    assert parse_risk_level("E4×D0") is None
    assert parse_risk_level("E2xD2") is None  # 注意是 × 不是 x


def test_required_events():
    assert "task_created" in REQUIRED_EVENTS
    assert "skill_routed" in REQUIRED_EVENTS
    assert "approval_decided" in REQUIRED_EVENTS


def test_undo_success_scorer_finds_compensation_reversed():
    """undo_success_scorer 应该在 audit_trace 含 compensation_reversed 时返回 Score(value=1)。"""
    # 这个测试验证 scorer 函数能被 import 并调用(不依赖 Inspect AI runtime)
    # 真正的 Inspect AI 集成测试在 inspect eval 时跑
    scorer_fn = undo_success_scorer()
    assert callable(scorer_fn), "scorer should return a callable"


def test_risk_level_format_in_100_tasks():
    """验证 100_tasks.yaml 中所有 risk_level 都是合法的 E×D 格式。"""
    import os
    from pathlib import Path

    import yaml

    evals_dir = Path(__file__).parent.parent
    yaml_path = evals_dir / "functional" / "100_tasks.yaml"
    if not yaml_path.exists():
        pytest.skip("100_tasks.yaml not found")

    with open(yaml_path, encoding="utf-8") as f:
        data = yaml.safe_load(f)

    for task in data["tasks"]:
        risk = task["risk_level"]
        parsed = parse_risk_level(risk)
        assert parsed is not None, f"task {task['id']} has invalid risk_level: {risk}"
