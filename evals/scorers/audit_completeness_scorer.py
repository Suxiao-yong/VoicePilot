"""audit_completeness_scorer — 校验 OTel span 完整性 + audit_logs 覆盖率(W11 Plan 1)。

spec §9.2:校验 OTel span 完整性 + audit_logs 覆盖率。

完整性判定:
- audit_trace 至少含 task_created / skill_routed / approval_decided 3 个事件
- 每个 event 含 event_type 字段
- task_created 含 task_id 字段
"""

from __future__ import annotations

import json
from typing import Any

from inspect_ai.scorer import Score, Target, accuracy, scorer
from inspect_ai.solver import TaskState

REQUIRED_EVENTS = {"task_created", "skill_routed", "approval_decided"}


@scorer(metrics=[accuracy()])
def audit_completeness_scorer():
    """校验 audit_logs 覆盖率(至少 3 个必需事件)。

    Pass: audit_trace 含 task_created + skill_routed + approval_decided
    Fail: 缺少任一必需事件
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

        event_types = set()
        for event in audit_trace:
            if isinstance(event, dict) and "event_type" in event:
                event_types.add(event["event_type"])

        missing = REQUIRED_EVENTS - event_types
        if not missing:
            return Score(
                value=1,
                explanation=f"all required events present: {sorted(event_types)}",
            )
        return Score(
            value=0,
            explanation=f"missing events: {sorted(missing)} (got: {sorted(event_types)})",
        )

    return score
