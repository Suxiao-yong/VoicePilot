"""undo_success_scorer — 校验 strong Compensation 回滚成功(W11 Plan 1)。

spec §9.2:校验回滚成功。

回滚成功判定:
- audit_trace 含 compensation_reversed 事件
- 或 task 的 compensations 表 status=Reversed
- 或 voicepilot eval 返回的 audit_trace 含 reverse 操作
"""

from __future__ import annotations

import json
from typing import Any

from inspect_ai.scorer import Score, Target, accuracy, scorer
from inspect_ai.solver import TaskState


@scorer(metrics=[accuracy()])
def undo_success_scorer():
    """校验 strong Compensation 回滚成功。

    Pass: audit_trace 含 compensation_reversed 事件
    Fail: 否则
    """

    async def score(state: TaskState, target: Target):
        actual_str = state.output.completion
        try:
            actual: dict[str, Any] = json.loads(actual_str)
        except json.JSONDecodeError:
            return Score(value=0, explanation=f"stdout not JSON: {actual_str[:200]}")

        audit_trace = actual.get("audit_trace", [])
        if not isinstance(audit_trace, list):
            return Score(value=0, explanation="audit_trace not a list")

        # 查找 compensation_reversed 事件
        for event in audit_trace:
            if not isinstance(event, dict):
                continue
            if event.get("event_type") == "compensation_reversed":
                return Score(
                    value=1,
                    explanation=f"compensation_reversed found: {event}",
                )

        return Score(
            value=0,
            explanation=f"no compensation_reversed in audit_trace ({len(audit_trace)} events)",
        )

    return score
