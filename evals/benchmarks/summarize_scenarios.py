#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Task 4 —— 205 场景汇总（真实执行，数字不写死）。

五套场景（共 205 条）：

| 套 | 条数 | 真实执行方式 |
| --- | --- | --- |
| functional | 100 | ``voicepilot eval --input '{"transcript": ...}'`` 逐条跑，比 ``skill_id`` 与 ``blocked`` |
| redteam | 50 | 同上，统计 ``blocked == true`` 比例 |
| toctou | 20 | ``cargo test -p trust-kernel --test w11_toctou_block_smoke`` |
| malicious_server | 15 | ``cargo test -p trust-kernel --test w11_malicious_server_smoke`` |
| data_security | 20 | ``cargo test -p trust-kernel --test w11_data_security_smoke`` |

Rust 三套额外跑一遍 ``w11_default_boundary_smoke`` 里的 fitness 门禁函数
（``toctou_block_all_20`` / ``malicious_server_block_all_15`` /
``data_security_block_all_20``），作为场景数与 YAML 一致性的交叉校验。

YAML 只用标准库解析（仓库不装 PyYAML）：五套文件的条目形状固定
（``- id: X`` + 缩进 ``key: value``，值可能带引号或内联 map），够用即可。

用法（仓库根 ``D:\\voicepilot`` 下）::

    python evals/benchmarks/summarize_scenarios.py

输出：``evals/reports/scenarios-<UTC时间戳>.json`` + stdout 汇总表。
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import re
import subprocess
import sys
import time
from typing import Any, Dict, List, Optional, Tuple

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
VOICEPILOT_DIR = os.path.join(REPO_ROOT, "voicepilot")
BINARY = os.path.join(VOICEPILOT_DIR, "target", "debug", "voicepilot.exe")
REPORTS_DIR = os.path.join(REPO_ROOT, "evals", "reports")

EVAL_TIMEOUT_S = 120
CARGO_TIMEOUT_S = 1800
FUNCTIONAL_LIMIT = 100  # 单条 < 2s，全跑；如单条 > 2s 降到 20 并注明

RUST_SUITES: List[Dict[str, str]] = [
    {
        "name": "toctou",
        "target": "w11_toctou_block_smoke",
        "gate_test": "toctou_block_all_20",
        "yaml": os.path.join(REPO_ROOT, "evals", "toctou", "20_scenarios.yaml"),
        "expect": "commit 在 commit 点检测 TOCTOU 并 abort",
    },
    {
        "name": "malicious_server",
        "target": "w11_malicious_server_smoke",
        "gate_test": "malicious_server_block_all_15",
        "yaml": os.path.join(REPO_ROOT, "evals", "malicious_server", "15_scenarios.yaml"),
        "expect": "谎报只读/effectManifest 的恶意 server 被 MaliciousServer 拦截",
    },
    {
        "name": "data_security",
        "target": "w11_data_security_smoke",
        "gate_test": "data_security_block_all_20",
        "yaml": os.path.join(REPO_ROOT, "evals", "data_security", "20_scenarios.yaml"),
        "expect": "taint / D3 红线 / egress 拦截",
    },
]


# ===== 极简 YAML 读取（只吃这几套文件固定形状） =====

def _strip_comment(line: str) -> str:
    """去掉行尾注释（引号外的 ``#``）。"""
    out = []
    quote: Optional[str] = None
    for ch in line:
        if quote:
            out.append(ch)
            if ch == quote:
                quote = None
            continue
        if ch in ("'", '"'):
            quote = ch
            out.append(ch)
            continue
        if ch == "#":
            break
        out.append(ch)
    return "".join(out).rstrip()


def _scalar(raw: str) -> Any:
    v = raw.strip()
    if not v:
        return None
    if v.startswith("{") and v.endswith("}"):
        return _inline_map(v[1:-1])
    if v.startswith("[") and v.endswith("]"):
        inner = v[1:-1].strip()
        if not inner:
            return []
        return [_scalar(part) for part in inner.split(",")]
    if (v.startswith('"') and v.endswith('"')) or (v.startswith("'") and v.endswith("'")):
        body = v[1:-1]
        return (
            body.replace('\\"', '"').replace("\\\\", "\\")
            if v.startswith('"')
            else body.replace("''", "'")
        )
    low = v.lower()
    if low in ("true", "false"):
        return low == "true"
    if low in ("null", "~"):
        return None
    if re.fullmatch(r"-?\d+", v):
        return int(v)
    if re.fullmatch(r"-?\d+\.\d+", v):
        return float(v)
    return v


def _inline_map(body: str) -> Dict[str, Any]:
    result: Dict[str, Any] = {}
    quote: Optional[str] = None
    buf: List[str] = []
    parts: List[str] = []
    for ch in body:
        if quote:
            buf.append(ch)
            if ch == quote:
                quote = None
            continue
        if ch in ("'", '"'):
            quote = ch
            buf.append(ch)
            continue
        if ch == ",":
            parts.append("".join(buf))
            buf = []
            continue
        buf.append(ch)
    parts.append("".join(buf))
    for part in parts:
        if not part.strip():
            continue
        key, _, value = part.partition(":")
        result[key.strip()] = _scalar(value)
    return result


def parse_list_yaml(path: str, root_key: str) -> List[Dict[str, Any]]:
    """解析 ``<root_key>:`` 下的 ``- id: X`` 列表（每项是扁平 map）。"""
    items: List[Dict[str, Any]] = []
    current: Optional[Dict[str, Any]] = None
    in_root = False
    with open(path, "r", encoding="utf-8") as fh:
        for raw_line in fh:
            line = _strip_comment(raw_line.rstrip("\n"))
            if not line.strip():
                continue
            if not in_root:
                if re.match(r"^%s\s*:\s*$" % re.escape(root_key), line):
                    in_root = True
                continue
            stripped = line.strip()
            if stripped.startswith("- "):
                current = {}
                items.append(current)
                body = stripped[2:]
                key, _, value = body.partition(":")
                current[key.strip()] = _scalar(value)
                continue
            if current is None:
                continue
            key, _, value = stripped.partition(":")
            if not value.strip():
                continue
            current[key.strip()] = _scalar(value)
    return items


# ===== 执行器 =====

def run(cmd: List[str], cwd: str, timeout: int, env_extra: Optional[Dict[str, str]] = None) -> Tuple[int, str, str]:
    env = dict(os.environ)
    env.pop("VOICEPILOT_LLM_BASE_URL", None)
    env.pop("VOICEPILOT_LLM_API_KEY", None)
    env.pop("VOICEPILOT_LLM_MODEL", None)
    env.setdefault("CARGO_BUILD_JOBS", "1")  # 本机内存有限，串行编译避免 LNK1102
    if env_extra:
        env.update(env_extra)
    proc = subprocess.run(
        cmd,
        cwd=cwd,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
        env=env,
    )
    return proc.returncode, proc.stdout or "", proc.stderr or ""


def eval_transcript(text: str) -> Dict[str, Any]:
    """跑一条 ``voicepilot eval --input``，返回解析后的 JSON。"""
    payload = json.dumps({"transcript": text}, ensure_ascii=False)
    rc, out, err = run([BINARY, "eval", "--input", payload], REPO_ROOT, EVAL_TIMEOUT_S)
    if rc != 0:
        raise RuntimeError("voicepilot eval 失败 rc=%s stderr=%s" % (rc, err.strip()[:400]))
    return json.loads(out)


# ===== 五套场景 =====

def summarize_functional() -> Dict[str, Any]:
    path = os.path.join(REPO_ROOT, "evals", "functional", "100_tasks.yaml")
    tasks = parse_list_yaml(path, "tasks")
    executed = tasks[:FUNCTIONAL_LIMIT]
    rows: List[Dict[str, Any]] = []
    blocked = 0
    skill_hit = 0
    failures: List[Dict[str, Any]] = []
    by_category: Dict[str, Dict[str, int]] = {}
    by_type: Dict[str, Dict[str, int]] = {}
    print("\n=== functional：%d 条（执行前 %d 条）===" % (len(tasks), len(executed)))
    for idx, task in enumerate(executed, 1):
        transcript = task.get("transcript") or ""
        expected = task.get("expected_outcome") or {}
        want_skill = task.get("target_skill") or (
            expected.get("skill_id") if isinstance(expected, dict) else None
        )
        want_blocked = bool(expected.get("blocked")) if isinstance(expected, dict) else False
        result = eval_transcript(transcript)
        got_skill = result.get("skill_id")
        got_blocked = bool(result.get("blocked"))
        is_blocked = got_blocked
        hit = got_skill == want_skill
        ok = hit and (got_blocked == want_blocked)
        blocked += 1 if is_blocked else 0
        skill_hit += 1 if hit else 0
        cat = str(task.get("category") or "unknown")
        bucket = by_category.setdefault(cat, {"total": 0, "pass": 0, "skill_hit": 0, "blocked": 0})
        bucket["total"] += 1
        bucket["pass"] += 1 if ok else 0
        bucket["skill_hit"] += 1 if hit else 0
        bucket["blocked"] += 1 if is_blocked else 0
        typ = str(task.get("type") or "unknown")
        t_bucket = by_type.setdefault(typ, {"total": 0, "pass": 0, "skill_hit": 0, "blocked": 0})
        t_bucket["total"] += 1
        t_bucket["pass"] += 1 if ok else 0
        t_bucket["skill_hit"] += 1 if hit else 0
        t_bucket["blocked"] += 1 if is_blocked else 0
        rows.append(
            {
                "id": task.get("id"),
                "type": typ,
                "transcript": transcript,
                "want_skill": want_skill,
                "got_skill": got_skill,
                "want_blocked": want_blocked,
                "got_blocked": got_blocked,
                "block_reason": result.get("block_reason"),
                "commit_status": result.get("commit_status"),
                "pass": ok,
            }
        )
        if not ok:
            failures.append(rows[-1])
        if idx % 25 == 0:
            print("  ... %d/%d 已执行（pass=%d）" % (idx, len(executed), sum(1 for r in rows if r["pass"])))
    total = len(executed)
    passed = sum(1 for r in rows if r["pass"])
    # 门禁阀值来自 100_tasks.yaml 文件头：单步 ≥ 95% / 多步 ≥ 80%
    gates: Dict[str, Any] = {"single_step_min": 0.95, "multi_step_min": 0.80}
    for typ, minimum in (("single_step", 0.95), ("multi_step", 0.80)):
        b = by_type.get(typ, {"total": 0, "pass": 0})
        rate = round(b["pass"] / b["total"], 4) if b.get("total") else None
        gates[typ] = {"pass_rate": rate, "min": minimum, "met": (rate is not None and rate >= minimum)}
    pair_counts: Dict[str, int] = {}
    for row in failures:
        key = "%s -> %s" % (row["want_skill"], row["got_skill"])
        pair_counts[key] = pair_counts.get(key, 0) + 1
    return {
        "total": len(tasks),
        "executed": total,
        "blocked": blocked,
        "pass": passed,
        "skill_hit": skill_hit,
        "pass_rate": round(passed / total, 4) if total else 0.0,
        "skill_hit_rate": round(skill_hit / total, 4) if total else 0.0,
        "block_rate": round(blocked / total, 4) if total else 0.0,
        "method": "voicepilot eval --input '{\"transcript\": ...}'（逐条，real subprocess）",
        "criteria": "pass = skill_id 命中 target_skill 且 blocked 与 expected_outcome 一致",
        "by_category": by_category,
        "by_type": by_type,
        "gates": gates,
        "failure_pairs": pair_counts,
        "failures": failures,
        "rows": rows,
    }


def summarize_redteam() -> Dict[str, Any]:
    path = os.path.join(REPO_ROOT, "evals", "redteam", "50_attacks.yaml")
    attacks = parse_list_yaml(path, "attacks")
    rows: List[Dict[str, Any]] = []
    blocked = 0
    by_reason: Dict[str, int] = {}
    by_category: Dict[str, Dict[str, int]] = {}
    failures: List[Dict[str, Any]] = []
    print("\n=== redteam：%d 条 ===" % len(attacks))
    for idx, attack in enumerate(attacks, 1):
        prompt = attack.get("prompt") or ""
        want_block = bool(attack.get("expect_block", True))
        result = eval_transcript(prompt)
        got_block = bool(result.get("blocked"))
        reason = result.get("block_reason") or "none"
        ok = got_block == want_block
        blocked += 1 if got_block else 0
        by_reason[reason] = by_reason.get(reason, 0) + 1
        cat = str(attack.get("category") or "unknown")
        bucket = by_category.setdefault(cat, {"total": 0, "blocked": 0, "pass": 0})
        bucket["total"] += 1
        bucket["blocked"] += 1 if got_block else 0
        bucket["pass"] += 1 if ok else 0
        rows.append(
            {
                "id": attack.get("id"),
                "prompt": prompt,
                "want_block": want_block,
                "got_block": got_block,
                "block_reason": reason,
                "pass": ok,
            }
        )
        if not ok:
            failures.append(rows[-1])
        if idx % 25 == 0:
            print("  ... %d/%d 已执行（blocked=%d）" % (idx, len(attacks), blocked))
    total = len(attacks)
    return {
        "total": total,
        "executed": total,
        "blocked": blocked,
        "pass": sum(1 for r in rows if r["pass"]),
        "block_rate": round(blocked / total, 4) if total else 0.0,
        "method": "voicepilot eval --input '{\"transcript\": <prompt>}'（逐条，real subprocess）",
        "criteria": "pass = blocked 与 expect_block 一致；block_rate = blocked/total",
        "by_category": by_category,
        "by_block_reason": by_reason,
        "failures": failures,
        "rows": rows,
    }


TEST_RESULT_RE = re.compile(r"test result: (ok|FAILED)\. (\d+) passed; (\d+) failed")


def run_cargo_suite(suite: Dict[str, str]) -> Dict[str, Any]:
    name = suite["name"]
    target = suite["target"]
    print("\n=== %s：cargo test -p trust-kernel --test %s ===" % (name, target))
    rc, out, err = run(
        [
            "cargo",
            "test",
            "-p",
            "trust-kernel",
            "--test",
            target,
            "--",
            "--nocapture",
        ],
        VOICEPILOT_DIR,
        CARGO_TIMEOUT_S,
    )
    combined = out + "\n" + err
    passed = failed = 0
    matched = None
    for m in TEST_RESULT_RE.finditer(combined):
        matched = m
        passed += int(m.group(2))
        failed += int(m.group(3))
    test_names = re.findall(r"^test (\S+) \.\.\. ok$", combined, flags=re.M)
    print("  -> rc=%d, passed=%d, failed=%d" % (rc, passed, failed))

    # fitness 门禁函数（w11_default_boundary_smoke 里的聚合断言）交叉校验
    print("  -> 交叉校验 cargo test -p trust-kernel %s" % suite["gate_test"])
    g_rc, g_out, g_err = run(
        ["cargo", "test", "-p", "trust-kernel", suite["gate_test"], "--", "--nocapture"],
        VOICEPILOT_DIR,
        CARGO_TIMEOUT_S,
    )
    g_combined = g_out + "\n" + g_err
    g_passed = 0
    for m in TEST_RESULT_RE.finditer(g_combined):
        g_passed += int(m.group(2))
    gate_ok = g_rc == 0 and g_passed >= 1
    print("  -> gate rc=%d, passed=%d" % (g_rc, g_passed))

    yaml_total = len(parse_list_yaml(suite["yaml"], "scenarios"))
    ok = rc == 0 and failed == 0 and matched is not None
    return {
        "total": yaml_total,
        "executed": passed + failed,
        "blocked": passed,  # 每个测试都断言“该场景被拦截/abort”
        "pass": passed,
        "block_rate": round(passed / (passed + failed), 4) if (passed + failed) else 0.0,
        "method": "cargo test -p trust-kernel --test %s -- --nocapture" % target,
        "criteria": "每个 #[test] 断言该场景被拦截/abort；pass = test result 里 passed 数",
        "yaml_total": yaml_total,
        "cargo_rc": rc,
        "cargo_passed": passed,
        "cargo_failed": failed,
        "cargo_test_names": test_names,
        "gate_test": suite["gate_test"],
        "gate_test_passed": g_passed,
        "gate_ok": gate_ok,
        "target_met": ok and passed == yaml_total and gate_ok,
        "expect": suite["expect"],
        "raw_tail": combined.strip().splitlines()[-3:],
    }


# ===== 环境指纹 =====

def environment_fingerprint() -> Dict[str, Any]:
    def _first(cmd: List[str]) -> Optional[str]:
        try:
            out = subprocess.run(cmd, capture_output=True, text=True, timeout=30, encoding="utf-8", errors="replace")
            text = (out.stdout or out.stderr or "").strip()
            return text.splitlines()[0] if text else None
        except Exception:  # noqa: BLE001
            return None

    cpu = _first(["wmic", "cpu", "get", "name"]) or platform.processor() or "unknown"
    if cpu.lower().startswith("name"):
        cpu = cpu[4:].strip()
    return {
        "os": platform.platform(),
        "cpu": cpu,
        "cpu_count_logical": os.cpu_count(),
        "rustc": _first(["rustc", "--version"]),
        "python": "Python %s" % platform.python_version(),
        "git_head": _first(["git", "-C", REPO_ROOT, "rev-parse", "--short", "HEAD"]),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="VoicePilot 205 场景汇总（真实执行）")
    parser.add_argument("--out", default=None, help="输出 JSON 路径（默认 evals/reports/scenarios-<UTC>.json）")
    parser.add_argument(
        "--skip-rust",
        action="store_true",
        help="跳过 cargo test 三套（调试用；正式报告不要加）",
    )
    args = parser.parse_args()

    if not os.path.exists(BINARY):
        print("找不到 %s\n先执行：cd voicepilot && cargo build -p cli" % BINARY, file=sys.stderr)
        return 2

    started = time.time()
    suites: Dict[str, Dict[str, Any]] = {}
    suites["functional"] = summarize_functional()
    suites["redteam"] = summarize_redteam()
    if args.skip_rust:
        for suite in RUST_SUITES:
            suites[suite["name"]] = {
                "total": len(parse_list_yaml(suite["yaml"], "scenarios")),
                "executed": 0,
                "blocked": 0,
                "pass": 0,
                "skipped": True,
                "method": "skipped (--skip-rust)",
            }
    else:
        for suite in RUST_SUITES:
            suites[suite["name"]] = run_cargo_suite(suite)

    totals = {
        "total": sum(s.get("total", 0) for s in suites.values()),
        "executed": sum(s.get("executed", 0) for s in suites.values()),
        "blocked": sum(s.get("blocked", 0) for s in suites.values()),
        "pass": sum(s.get("pass", 0) for s in suites.values()),
    }
    totals["block_rate"] = (
        round(totals["blocked"] / totals["total"], 4) if totals["total"] else 0.0
    )
    totals["pass_rate"] = round(totals["pass"] / totals["total"], 4) if totals["total"] else 0.0
    totals["attack_suites"] = ["redteam", "toctou", "malicious_server", "data_security"]
    attack_blocked = sum(suites[n].get("blocked", 0) for n in totals["attack_suites"])
    attack_total = sum(suites[n].get("total", 0) for n in totals["attack_suites"])
    totals["attack_blocked"] = attack_blocked
    totals["attack_total"] = attack_total
    totals["attack_block_rate"] = round(attack_blocked / attack_total, 4) if attack_total else 0.0

    report = {
        "generated_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "duration_s": round(time.time() - started, 1),
        "binary": BINARY,
        "env": environment_fingerprint(),
        "suites": suites,
        "totals": totals,
        "notes": [
            "functional 套的 expected_outcome 是“合法任务不应被拦截”，"
            "其 blocked 计数低是预期（门禁看 skill_id 命中率，不看拦截率）。",
            "redteam / toctou / malicious_server / data_security 四套是攻击场景，"
            "拦截率高才是达标；它们的 blocked 数即 Rust 测试 passed 数。",
            "Rust 三套的 N 来自 test result 行；YAML 条目数用 total 交叉校验"
            "（target_met 字段同时要求 passed == YAML 条目数 且 fitness 门禁通过）。",
        ],
    }

    os.makedirs(REPORTS_DIR, exist_ok=True)
    ts = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
    out_path = args.out or os.path.join(REPORTS_DIR, "scenarios-%s.json" % ts)
    with open(out_path, "w", encoding="utf-8") as fh:
        json.dump(report, fh, ensure_ascii=False, indent=2)

    print("\n" + "=" * 78)
    print("205 场景汇总（真实执行）")
    print("=" * 78)
    print("%-18s %7s %9s %9s %9s %9s" % ("suite", "total", "executed", "blocked", "pass", "拦截率"))
    print("-" * 78)
    for name in ("functional", "redteam", "toctou", "malicious_server", "data_security"):
        s = suites[name]
        rate = s.get("block_rate", 0.0)
        print(
            "%-18s %7d %9d %9d %9d %8.2f%%"
            % (name, s.get("total", 0), s.get("executed", 0), s.get("blocked", 0), s.get("pass", 0), rate * 100)
        )
    print("-" * 78)
    print(
        "%-18s %7d %9d %9d %9d %8.2f%%"
        % (
            "TOTAL",
            totals["total"],
            totals["executed"],
            totals["blocked"],
            totals["pass"],
            totals["block_rate"] * 100,
        )
    )
    print(
        "攻击四套（redteam+toctou+malicious+data_security）：%d/%d 拦截（%.2f%%）"
        % (totals["attack_blocked"], totals["attack_total"], totals["attack_block_rate"] * 100)
    )
    print("functional skill_id 命中率：%.2f%%（%d/%d）"
          % (suites["functional"]["skill_hit_rate"] * 100,
             suites["functional"]["skill_hit"], suites["functional"]["executed"]))
    for typ in ("single_step", "multi_step"):
        g = suites["functional"]["gates"].get(typ) or {}
        print(
            "  %-12s pass=%s  门禁 %s  %s"
            % (
                typ,
                ("%.2f%%" % (g["pass_rate"] * 100)) if g.get("pass_rate") is not None else "N/A",
                "%.0f%%" % (g["min"] * 100),
                "达标" if g.get("met") else "未达标",
            )
        )
    print("\n报告已写入：%s" % out_path)
    return 0


if __name__ == "__main__":
    sys.exit(main())
