"""VoicePilot V1 评测 — Inspect AI Task 定义(W11 Plan 1)。

加载 evals/functional/100_tasks.yaml,每个任务作为一个 Sample,
调用 voicepilot eval --input <json>,scorer 校验返回的 JSON。

运行:
    cd d:\\voicepilot\\evals
    inspect eval inspect_evals.py --limit 10  # 跑前 10 个
    inspect eval inspect_evals.py              # 跑全部 100 个
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any

import yaml
from inspect_ai import Task, task
from inspect_ai.dataset import Sample
from inspect_ai.scorer import Score, Target, accuracy, scorer
from inspect_ai.solver import Generate, TaskState, generate

# VoicePilot 二进制路径(target/debug/voicepilot.exe)
EVALS_DIR = Path(__file__).parent
PROJECT_ROOT = EVALS_DIR.parent
VOICEPILOT_BIN = PROJECT_ROOT / "voicepilot" / "target" / "debug" / "voicepilot.exe"

# 100 任务 YAML 路径
TASKS_YAML = EVALS_DIR / "functional" / "100_tasks.yaml"


def load_tasks() -> list[dict[str, Any]]:
    """加载 100_tasks.yaml,返回任务列表。"""
    with open(TASKS_YAML, encoding="utf-8") as f:
        data = yaml.safe_load(f)
    return data["tasks"]


def call_voicepilot_eval(transcript: str, mode: str = "auto") -> dict[str, Any]:
    """调用 voicepilot eval --input <json>,返回解析后的 JSON 结果。

    Args:
        transcript: 用户语音转写文本
        mode: auto | interactive

    Returns:
        VoicePilot 输出的 JSON 结果(含 task_id / skill_id / commit_status / 等)
    """
    input_json = json.dumps({"transcript": transcript, "mode": mode}, ensure_ascii=False)
    db_path = PROJECT_ROOT / "evals" / "reports" / f"eval_{os.getpid()}.db"
    db_path.parent.mkdir(parents=True, exist_ok=True)

    result = subprocess.run(
        [str(VOICEPILOT_BIN), "eval", "--input", input_json],
        env={**os.environ, "VOICEPILOT_DB": str(db_path)},
        capture_output=True,
        text=True,
        timeout=30,
        cwd=str(PROJECT_ROOT),
    )

    if result.returncode != 0:
        return {
            "error": f"voicepilot eval failed: {result.stderr}",
            "blocked": True,
            "block_reason": "process_error",
            "commit_status": "failed",
        }

    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as e:
        return {
            "error": f"stdout not JSON: {e}: {result.stdout[:200]}",
            "blocked": True,
            "block_reason": "json_decode_error",
            "commit_status": "failed",
        }


@scorer(metrics=[accuracy()])
def voicepilot_functional_scorer():
    """校验 voicepilot eval 返回的 skill_id 与 expected_outcome 匹配。

    Pass: skill_id 匹配 expected_outcome.skill_id 且 commit_status 匹配
    Fail: 否则
    """
    async def score(state: TaskState, target: Target):
        expected = json.loads(target.text)
        actual_str = state.output.completion
        try:
            actual = json.loads(actual_str)
        except json.JSONDecodeError:
            return Score(value=0, explanation=f"stdout not JSON: {actual_str[:200]}")

        expected_skill = expected.get("skill_id")
        actual_skill = actual.get("skill_id")
        expected_status = expected.get("commit_status")
        actual_status = actual.get("commit_status")

        if actual_skill == expected_skill and actual_status == expected_status:
            return Score(value=1, explanation=f"matched: skill={actual_skill}, status={actual_status}")
        return Score(
            value=0,
            explanation=f"mismatch: expected skill={expected_skill}/status={expected_status}, "
            f"got skill={actual_skill}/status={actual_status}",
        )

    return score


@task
def voicepilot_functional() -> Task:
    """VoicePilot V1 功能任务评测(100 任务)。

    门禁:单步 ≥ 95%(≥ 48/50)+ 多步 ≥ 80%(≥ 40/50)。
    """
    tasks = load_tasks()
    samples = []
    for t in tasks:
        sample = Sample(
            id=t["id"],
            input=t["transcript"],
            target=json.dumps(t["expected_outcome"], ensure_ascii=False),
            metadata={
                "category": t["category"],
                "type": t["type"],
                "target_skill": t["target_skill"],
                "risk_level": t["risk_level"],
            },
        )
        samples.append(sample)

    async def solve(state: TaskState, generate: Generate) -> TaskState:
        """调用 voicepilot eval,把结果写入 state.output.completion。"""
        transcript = state.input_text
        result = call_voicepilot_eval(transcript, mode="auto")
        state.output.completion = json.dumps(result, ensure_ascii=False)
        return state

    return Task(
        dataset=samples,
        solver=[solve],
        scorer=voicepilot_functional_scorer(),
        sandbox="local",
    )


if __name__ == "__main__":
    # self-test: 跑 1 个 sample 验证 scorer 工作
    print("self-test: loading tasks...")
    tasks = load_tasks()
    print(f"loaded {len(tasks)} tasks")
    print(f"first task: {tasks[0]['id']} - {tasks[0]['transcript']}")

    print("\nself-test: calling voicepilot eval...")
    if not VOICEPILOT_BIN.exists():
        print(f"ERROR: voicepilot binary not found at {VOICEPILOT_BIN}")
        print("Run: cargo build --manifest-path voicepilot\\Cargo.toml -p cli")
        sys.exit(1)

    result = call_voicepilot_eval(tasks[0]["transcript"])
    print(f"result: {json.dumps(result, indent=2, ensure_ascii=False)}")
