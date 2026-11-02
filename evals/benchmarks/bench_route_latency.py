#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Task 4 —— 路由延迟基准（真实实测，纯标准库）。

测四组路由通道的 HTTP 往返延迟（`POST /api/v1/route`，wall-clock）：

1. ``fastroute`` —— 20 条**来自** ``crates/trust-kernel/src/skills/fastroute.rs``
   单测用例的句式（``match_fast_route`` 能填满参数的形状），断言
   ``route_kind == "fastroute"``；不符者剔除并告警（绝不让 keyword 混进本组）。
2. ``keyword`` —— 20 条命中 ``SkillRouter`` 关键词表的句式，断言
   ``route_kind == "keyword"``（且 ``kind == "skill"``，否则该句式不算“关键词路由命中”）。
3. ``llm`` —— 20 条**不命中**快路由/关键词的句式，经本地 OpenAI 兼容 stub
   （``POST /chat/completions`` 固定 sleep 1.2s 后返回 tool_calls）走完整
   classify 链路，断言 ``route_kind == "llm"`` 且 ``used_llm == true``。
   **这是本地 stub 模拟的云端往返，不是真实云端 API。**
4. ``cache`` —— 同一批 LLM 句式再跑 5 轮，第二轮起应命中 route cache
   （``route_kind == "cache"``）；未命中的轮次如实记录，不伪造。

每组每句跑 5 轮，**首轮丢弃**（冷启动 / 进程预热），只统计后 4 轮；用
``time.perf_counter()`` 记录 HTTP 往返耗时。

交叉校验：跑完后用 ``sqlite3`` 直读临时 DB 的 ``voice_latency_samples``，
按 ``route_kind`` 重算 P50/P95，与 HTTP 侧（**全轮次**，含被丢弃的首轮，
以保证与 DB 行集一一对应）对比；差异 > 5% 在报告里标红（``flagged``）并说明原因。

用法（在仓库根 ``D:\\voicepilot`` 下）::

    python evals/benchmarks/bench_route_latency.py

输出：``evals/reports/bench-<UTC时间戳>.json`` + stdout 汇总表。
"""

from __future__ import annotations

import argparse
import atexit
import json
import math
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any, Dict, List, Optional, Tuple

if hasattr(sys.stdout, "reconfigure"):  # GBK 控制台也能打印中文
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

# ===== 路径 =====

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
VOICEPILOT_DIR = os.path.join(REPO_ROOT, "voicepilot")
BINARY = os.path.join(VOICEPILOT_DIR, "target", "debug", "voicepilot.exe")
REPORTS_DIR = os.path.join(REPO_ROOT, "evals", "reports")

ROUNDS = 5              # 每句轮数（首轮丢弃）
ROUNDS_MEASURED = 4     # 实际计入统计的轮数
STUB_DELAY_S = 1.2      # LLM stub 固定往返
SERVE_START_TIMEOUT_S = 90.0
HTTP_TIMEOUT_S = 120.0

# ===== 句式集（全部有出处，不是编的）=====

# 20 条覆盖 20 个不同 skill 的快路由句式：逐条抄自
# crates/trust-kernel/src/skills/fastroute.rs 的 mod fastroute_tests 用例
# （open_url_bare_and_verb_forms / scrape_needs_url_and_verb /
#  research_save_needs_url_and_path / download_detects_audio_flag /
#  fetch_file_routes_software_queries / media_trim_convert_shapes /
#  doc_office_create_shape / fs_write_splits_content_and_path /
#  fs_create_needs_explicit_path / fs_read_list_delete_need_path_shape /
#  clip_type_basic_shapes / press_keys_maps_chinese_names /
#  shell_takes_remainder / volume_playback_fixed_vocab / timer_parses_units /
#  alarm_and_reminder_split / page_screenshot_bare_form_only）。
FASTROUTE_TEXTS: List[str] = [
    "打开 https://example.com/a",              # sys.open_url
    "抓取https://example.com/a",                # web.scrape
    "把https://example.com/a存到Documents/x.md",  # research.save_markdown
    "下载https://x.com/a.mp4",                  # media.download
    "下载微信安装包",                            # web.fetch_file
    "把D:/a.mp4从00:10剪到00:20",               # media.trim_video
    "把D:/a.mp4转成mp3",                        # media.convert_video
    "把D:/a.docx转成pdf",                       # doc.convert
    "新建word文档：会议纪要",                     # doc.office
    "把hello写入D:/a.txt",                      # fs.write_file
    "新建文件D:/a.txt",                         # fs.create_file
    "删除D:/a.txt",                             # fs.delete_file
    "读D:/a.txt文件",                           # fs.read_file
    "列一下D:/work目录",                        # fs.list_dir
    "输入hello world",                          # clip.type_text
    "按回车",                                   # clip.press_keys
    "运行命令dir",                              # shell.run
    "把声音调高",                                # sys.volume
    "倒计时5分钟",                              # sys.timer_set
    "下午3点提醒我开会",                          # pim.reminder_create
]

# 20 条命中 SkillRouter 关键词表的句式（关键词来源：
# crates/trust-kernel/src/skills/manifest.rs 各 manifest 的 `keywords`：
# files.organize 整理/归档/移动文件/下载目录；task.explain 解释/刚才/上一步；
# task.compensate 撤销/回滚/补偿；form.prepare 表单/填充/准备；
# research.save_markdown 网页/Markdown/抓取；pim.note_create 记事本/记录/笔记；
# task.repeat_verified 重做/重复/再来一次）。
KEYWORD_TEXTS: List[str] = [
    "整理下载目录",                              # files.organize
    "把下载目录里的 PDF 整理到论文文件夹",          # files.organize
    "帮我整理一下桌面文件",                        # files.organize
    "归档这些照片到相册目录",                      # files.organize
    "移动文件到备份盘",                            # files.organize
    "把下载目录归档",                              # files.organize
    "解释一下刚才那一步",                          # task.explain
    "刚才那个操作做了什么",                        # task.explain
    "上一步是什么",                                # task.explain
    "解释刚才的操作",                              # task.explain
    "撤销刚才的操作",                              # task.compensate
    "回滚上一个动作",                              # task.compensate
    "补偿一下刚才的删除",                          # task.compensate
    "帮我填这个表单",                              # form.prepare
    "帮我准备表单",                                # form.prepare
    "准备一下报销表单",                            # form.prepare
    "把这个网页存为 Markdown",                     # research.save_markdown
    "在记事本里记一条笔记",                        # pim.note_create
    "再来一次刚才的操作",                          # task.repeat_verified
    "重做刚才那步",                                # task.repeat_verified
]

# 20 条既不命中快路由也不命中关键词的自然语言指令（落 LLM classify）。
# 已逐条实测确认：无 LLM 时均为 route_kind=keyword + kind=unmatched，
# 配 stub 后为 route_kind=llm + used_llm=true。
LLM_TEXTS: List[str] = [
    "帮我把季度报表按部门分一下",
    "请讲解量子计算原理",
    "帮我写一份周报总结",
    "把上季度的销售数据分析一下",
    "总结一下这份合同的风险点",
    "帮我想三个视频标题",
    "把这段英文翻译成日语",
    "分析一下这台电脑的磁盘占用",
    "帮我规划下周的学习计划",
    "分析今天的会议记录要点",
    "帮我想个产品名字",
    "对比一下这两个方案的优劣",
    "把这篇论文的核心结论提取出来",
    "帮我把这段代码优化一下",
    "推荐几本机器学习入门书",
    "帮我总结今天的对话",
    "这个月的支出都花在哪里了",
    "帮我分析一下竞争对手的产品定价",
    "帮我起草一份合作意向书",
    "把这几个方案排个优先级",
]

# LLM stub 的响应体：形状照抄 crates/trust-kernel/src/llm/client.rs 的
# `classify_and_extract_parses_valid_response` 用例，confidence 抬到 0.95
# （>= planner 的 0.7 阈值，确保走 Skill 分支而不是 DAG 拆解）。
STUB_BODY: Dict[str, Any] = {
    "choices": [
        {
            "message": {
                "role": "assistant",
                "tool_calls": [
                    {
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "route_skill",
                            "arguments": json.dumps(
                                {
                                    "matched_skill_id": "files.organize",
                                    "confidence": 0.95,
                                    "slots": [
                                        {
                                            "kind": "path",
                                            "raw": "C:\\Downloads",
                                            "high_risk": True,
                                        }
                                    ],
                                    "reasoning": "user wants to organize files",
                                },
                                ensure_ascii=False,
                            ),
                        },
                    }
                ],
            }
        }
    ]
}


# ===== 工具函数 =====

_LIVE_PROCS: List[subprocess.Popen] = []


def _kill_all() -> None:
    for proc in _LIVE_PROCS:
        try:
            if proc.poll() is None:
                proc.kill()
                proc.wait(timeout=10)
        except Exception:  # noqa: BLE001 - 清理路径不允许抛
            pass


atexit.register(_kill_all)


def percentile(sorted_vals: List[float], q: float) -> Optional[float]:
    """线性插值分位（与 numpy 默认口径一致，DB 侧复用同一实现）。"""
    if not sorted_vals:
        return None
    if len(sorted_vals) == 1:
        return float(sorted_vals[0])
    pos = (len(sorted_vals) - 1) * q
    lo, hi = math.floor(pos), math.ceil(pos)
    if lo == hi:
        return float(sorted_vals[int(pos)])
    return float(sorted_vals[lo] + (sorted_vals[hi] - sorted_vals[lo]) * (pos - lo))


def stats_block(values: List[float]) -> Dict[str, Any]:
    if not values:
        return {"count": 0}
    ordered = sorted(values)
    return {
        "count": len(ordered),
        "min_ms": round(ordered[0], 3),
        "max_ms": round(ordered[-1], 3),
        "mean_ms": round(sum(ordered) / len(ordered), 3),
        "p50_ms": round(percentile(ordered, 0.50), 3),
        "p95_ms": round(percentile(ordered, 0.95), 3),
        "p99_ms": round(percentile(ordered, 0.99), 3),
    }


# ===== serve 进程 =====

class ServeInstance:
    """一个 `voicepilot serve --port 0 --db <临时库>` 子进程。"""

    def __init__(self, db_path: str, env_extra: Optional[Dict[str, str]] = None) -> None:
        self.db_path = db_path
        self.port: Optional[int] = None
        self._log_path = db_path + ".serve.log"
        self._log = open(self._log_path, "w", encoding="utf-8", errors="replace")
        env = dict(os.environ)
        env.pop("VOICEPILOT_LLM_BASE_URL", None)
        env.pop("VOICEPILOT_LLM_API_KEY", None)
        env.pop("VOICEPILOT_LLM_MODEL", None)
        if env_extra:
            env.update(env_extra)
        self.proc = subprocess.Popen(
            [BINARY, "serve", "--host", "127.0.0.1", "--port", "0", "--db", db_path],
            stdout=subprocess.PIPE,
            stderr=self._log,
            text=True,
            encoding="utf-8",
            errors="replace",
            env=env,
            bufsize=1,
        )
        _LIVE_PROCS.append(self.proc)

    def wait_ready(self) -> int:
        assert self.proc.stdout is not None
        deadline = time.time() + SERVE_START_TIMEOUT_S
        while time.time() < deadline:
            line = self.proc.stdout.readline()
            if not line:
                if self.proc.poll() is not None:
                    raise RuntimeError(
                        "voicepilot serve 提前退出（rc=%s），日志见 %s"
                        % (self.proc.returncode, self._log_path)
                    )
                time.sleep(0.05)
                continue
            if "voicepilot serve listening on http://" in line:
                self.port = int(line.rsplit(":", 1)[1].strip())
                return self.port
        raise RuntimeError("等待 serve 端口超时（%ss）" % SERVE_START_TIMEOUT_S)

    def stop(self) -> None:
        try:
            if self.proc.poll() is None:
                self.proc.terminate()
                try:
                    self.proc.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    self.proc.kill()
                    self.proc.wait(timeout=10)
        finally:
            try:
                self._log.close()
            except Exception:  # noqa: BLE001
                pass
            try:
                if self in _LIVE_PROCS:
                    _LIVE_PROCS.remove(self.proc)
            except ValueError:
                pass


def route_once(port: int, text: str) -> Tuple[Dict[str, Any], float]:
    """POST /api/v1/route，返回 (响应 JSON, wall-clock 毫秒)。"""
    payload = json.dumps({"text": text}, ensure_ascii=False).encode("utf-8")
    req = urllib.request.Request(
        "http://127.0.0.1:%d/api/v1/route" % port,
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    started = time.perf_counter()
    with urllib.request.urlopen(req, timeout=HTTP_TIMEOUT_S) as resp:
        raw = resp.read()
    elapsed_ms = (time.perf_counter() - started) * 1000.0
    return json.loads(raw.decode("utf-8")), elapsed_ms


# ===== OpenAI 兼容 stub =====

class StubHandler(BaseHTTPRequestHandler):
    delay_s = STUB_DELAY_S
    counter = 0
    lock = threading.Lock()

    def do_POST(self) -> None:  # noqa: N802 - http.server 命名
        length = int(self.headers.get("content-length") or 0)
        if length:
            self.rfile.read(length)
        time.sleep(self.delay_s)
        with StubHandler.lock:
            StubHandler.counter += 1
        body = json.dumps(STUB_BODY, ensure_ascii=False).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args: Any) -> None:  # 静音
        return


class LlmStub:
    """本地 OpenAI 兼容 stub（固定延迟模拟云端往返，**不是真实云端 API**）。"""

    def __init__(self, delay_s: float = STUB_DELAY_S) -> None:
        StubHandler.delay_s = delay_s
        StubHandler.counter = 0
        self.httpd = ThreadingHTTPServer(("127.0.0.1", 0), StubHandler)
        self.httpd.daemon_threads = True
        self.thread = threading.Thread(target=self.httpd.serve_forever, daemon=True)

    def __enter__(self) -> "LlmStub":
        self.thread.start()
        return self

    def __exit__(self, *exc: Any) -> None:
        self.httpd.shutdown()
        self.httpd.server_close()

    @property
    def port(self) -> int:
        return int(self.httpd.server_address[1])

    @property
    def base_url(self) -> str:
        return "http://127.0.0.1:%d" % self.port

    @property
    def calls(self) -> int:
        return StubHandler.counter

    def env(self) -> Dict[str, str]:
        return {
            "VOICEPILOT_LLM_BASE_URL": self.base_url,
            "VOICEPILOT_LLM_API_KEY": "sk-test",
            "VOICEPILOT_LLM_MODEL": "stub-model-1.2s",
        }


# ===== 单组测量 =====

def measure_group(
    name: str,
    texts: List[str],
    assertion_desc: str,
    check,
    new_serve,
    fresh_serve_per_round: bool = False,
) -> Dict[str, Any]:
    """跑一组：每句 ROUNDS 轮，首轮当预检（丢弃），后 4 轮计入统计。

    ``check(resp)`` 返回 (ok: bool, reason: str)。预检不通过的句式从本组剔除
    （打印告警），绝不把别的通道混进本组。

    ``fresh_serve_per_round=True`` 时每轮重建 serve 进程 + 全新临时 DB：route cache
    持久化在 DB 里，只有这样才能保证每轮都是真实的冷 cache LLM 往返。
    """
    print("\n=== 组 %s（%d 句 × %d 轮，首轮丢弃）===" % (name, len(texts), ROUNDS))
    print("    断言：%s" % assertion_desc)
    if fresh_serve_per_round:
        print("    隔离：每轮重建 serve + 全新临时 DB（避免 route cache 干扰）")
    measured: List[float] = []
    all_rounds: List[float] = []
    excluded: List[Dict[str, str]] = []
    anomalies: List[Dict[str, Any]] = []
    preflight_ok: Dict[str, bool] = {}

    current: Optional[Tuple[ServeInstance, Any]] = None
    try:
        for round_idx in range(1, ROUNDS + 1):
            if current is None or (round_idx > 1 and fresh_serve_per_round):
                if current is not None:
                    current[0].stop()
                    current[1]()
                current = new_serve()
            serve = current[0]
            for text in texts:
                resp, elapsed = route_once(serve.port, text)
                all_rounds.append(elapsed)
                ok, reason = check(resp)
                if round_idx == 1:
                    # 第 1 轮：预检 + 冷启动丢弃
                    preflight_ok[text] = ok
                    if not ok:
                        excluded.append(
                            {
                                "text": text,
                                "reason": reason,
                                "route_kind": resp.get("route_kind"),
                                "kind": resp.get("kind"),
                            }
                        )
                        print(
                            "  [WARN] 剔除句式 %r —— %s（实际 route_kind=%s kind=%s）"
                            % (text, reason, resp.get("route_kind"), resp.get("kind"))
                        )
                    continue
                if not ok:
                    anomalies.append(
                        {
                            "round": round_idx,
                            "text": text,
                            "reason": reason,
                            "route_kind": resp.get("route_kind"),
                        }
                    )
                    continue
                if preflight_ok.get(text):
                    measured.append(elapsed)
    finally:
        if current is not None:
            current[0].stop()
            current[1]()

    block = stats_block(measured)
    block.update(
        {
            "group": name,
            "samples_ms": [round(v, 3) for v in measured],
            "assertion": assertion_desc,
            "texts_total": len(texts),
            "texts_used": len([t for t in texts if preflight_ok.get(t)]),
            "rounds_total": ROUNDS,
            "rounds_measured": ROUNDS_MEASURED,
            "fresh_serve_per_round": fresh_serve_per_round,
            "excluded": excluded,
            "anomalies": anomalies,
            "all_rounds_ms": [round(v, 3) for v in all_rounds],
        }
    )
    block["target_met"] = not excluded and not anomalies
    return block


# ===== 主流程 =====

def run_benchmark(tmp_root: str) -> Dict[str, Any]:
    os.makedirs(REPORTS_DIR, exist_ok=True)
    groups: List[Dict[str, Any]] = []
    notes: List[str] = []
    cache_detail: Dict[str, Any] = {}

    # ---------- 组 1/2：fastroute + keyword（无 LLM，共用一个 serve/DB）----------
    shared_db = os.path.join(tmp_root, "shared.db")
    shared_state: Dict[str, Any] = {}

    def shared_serve():
        inst = ServeInstance(shared_db)
        inst.wait_ready()
        shared_state["db"] = shared_db
        return inst, lambda: None

    groups.append(
        measure_group(
            "fastroute",
            FASTROUTE_TEXTS,
            "route_kind == 'fastroute'",
            lambda r: (
                (True, "ok")
                if r.get("route_kind") == "fastroute"
                else (False, "route_kind 不是 fastroute")
            ),
            shared_serve,
        )
    )
    groups.append(
        measure_group(
            "keyword",
            KEYWORD_TEXTS,
            "route_kind == 'keyword' 且 kind == 'skill'",
            lambda r: (
                (True, "ok")
                if r.get("route_kind") == "keyword" and r.get("kind") == "skill"
                else (
                    False,
                    "route_kind/kind 不符（keyword 组要求真的关键词路由命中）",
                )
            ),
            shared_serve,
        )
    )

    # ---------- 组 3：llm（本地 stub 固定 1.2s）----------
    llm_dbs: List[str] = []
    with LlmStub(STUB_DELAY_S) as stub:
        llm_env = stub.env()
        calls_before = stub.calls

        def llm_serve():
            # 每轮用**全新临时 DB**：route cache 是持久化在 DB 里的，复用库会让
            # 第 2 轮起变成 cache 命中，测不到真实 LLM 往返。
            idx = len(llm_dbs) + 1
            db = os.path.join(tmp_root, "llm-round-%d.db" % idx)
            llm_dbs.append(db)
            inst = ServeInstance(db, llm_env)
            inst.wait_ready()
            return inst, lambda: None

        llm_group = measure_group(
            "llm",
            LLM_TEXTS,
            "route_kind == 'llm' 且 used_llm == true（本地 stub，固定 1.2s）",
            lambda r: (
                (True, "ok")
                if r.get("route_kind") == "llm" and r.get("used_llm") is True
                else (
                    False,
                    "route_kind/used_llm 不符（期望 llm + used_llm=true）",
                )
            ),
            llm_serve,
            fresh_serve_per_round=True,
        )
        llm_calls = stub.calls - calls_before
        llm_group["stub_delay_s"] = STUB_DELAY_S
        llm_group["stub_calls"] = llm_calls
        llm_group["stub_requests_expected"] = len(LLM_TEXTS) * ROUNDS
        groups.append(llm_group)

        # ---------- 组 4：cache（同一批 LLM 句式，第 2 轮起应命中 route cache）----------
        cache_db = os.path.join(tmp_root, "cache.db")
        cache_serve = ServeInstance(cache_db, llm_env)
        cache_serve.wait_ready()
        measured: List[float] = []
        all_rounds: List[float] = []
        warmup_ms: List[float] = []
        per_round_hits: List[Dict[str, Any]] = []
        anomalies: List[Dict[str, Any]] = []
        try:
            # 第 1 轮：预热（把 classify 结果写进 route cache），丢弃
            warmup_miss = 0
            for text in LLM_TEXTS:
                resp, elapsed = route_once(cache_serve.port, text)
                warmup_ms.append(elapsed)
                if resp.get("route_kind") != "llm":
                    warmup_miss += 1
            print(
                "\n=== 组 cache（%d 句 × %d 轮，第 1 轮预热填缓存后丢弃）==="
                % (len(LLM_TEXTS), ROUNDS)
            )
            print("    断言：route_kind == 'cache'")
            for round_idx in range(1, ROUNDS + 1):
                hits = 0
                llm_fallback = 0
                for text in LLM_TEXTS:
                    resp, elapsed = route_once(cache_serve.port, text)
                    all_rounds.append(elapsed)
                    kind = resp.get("route_kind")
                    if kind == "cache":
                        hits += 1
                        measured.append(elapsed)
                    elif kind == "llm":
                        llm_fallback += 1
                        anomalies.append(
                            {"round": round_idx, "text": text, "route_kind": kind}
                        )
                    else:
                        anomalies.append(
                            {"round": round_idx, "text": text, "route_kind": kind}
                        )
                per_round_hits.append(
                    {
                        "round": round_idx,
                        "hits": hits,
                        "total": len(LLM_TEXTS),
                        "llm_fallback": llm_fallback,
                    }
                )
                print(
                    "  第 %d 轮：cache 命中 %d/%d（回落 llm %d）"
                    % (round_idx, hits, len(LLM_TEXTS), llm_fallback)
                )
        finally:
            cache_serve.stop()

        cache_block = stats_block(measured)
        total_requests = len(LLM_TEXTS) * ROUNDS
        cache_block.update(
            {
                "group": "cache",
                "samples_ms": [round(v, 3) for v in measured],
                "assertion": "route_kind == 'cache'（第 1 轮预热后，5 轮全部命中）",
                "texts_total": len(LLM_TEXTS),
                "texts_used": len(LLM_TEXTS),
                "rounds_total": ROUNDS,
                "rounds_measured": ROUNDS,
                "fresh_serve_per_round": False,
                "excluded": [],
                "anomalies": anomalies,
                "warmup_ms": [round(v, 3) for v in warmup_ms],
                "all_rounds_ms": [round(v, 3) for v in all_rounds],
            }
        )
        cache_block["target_met"] = not anomalies
        groups.append(cache_block)

        cache_detail = {
            "per_round_hits": per_round_hits,
            "hit_requests": len(measured),
            "total_requests": total_requests,
            "hit_rate": round(len(measured) / total_requests, 4) if total_requests else 0.0,
            "warmup_requests_not_llm": warmup_miss,
            "stub_calls_in_cache_phase": stub.calls - calls_before - llm_calls,
            "expected_stub_calls_in_cache_phase": len(LLM_TEXTS),
        }
        if anomalies:
            notes.append(
                "cache 组未达标：%d/%d 次请求没有命中 route cache（见 anomalies），如实记录未伪造。"
                % (len(anomalies), total_requests)
            )

    # ---------- 交叉校验：直读 SQLite voice_latency_samples ----------
    db_map = {
        "fastroute": [shared_db],
        "keyword": [shared_db],
        "llm": llm_dbs,
        "cache": [cache_db],
    }
    cross_check = cross_validate(groups, db_map)

    # ---------- 汇总 ----------
    env = environment_fingerprint()
    report: Dict[str, Any] = {
        "generated_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "binary": BINARY,
        "binary_sha256": sha256_file(BINARY),
        "env": env,
        "methodology": {
            "endpoint": "POST /api/v1/route",
            "clock": "time.perf_counter()（HTTP 往返 wall-clock）",
            "rounds_per_text": ROUNDS,
            "first_round": "丢弃（冷启动/进程预热）",
            "percentile": "线性插值（numpy 默认口径），DB 侧复用同一实现",
            "llm_group": "本地 OpenAI 兼容 stub，POST /chat/completions 固定 sleep %.1fs"
            " —— 模拟云端往返，不是真实云端 API" % STUB_DELAY_S,
            "llm_group_cache_isolation": "llm 组每轮用全新临时 DB，避免 route cache 干扰",
            "cache_group": "第 1 轮预热填缓存并丢弃，随后 %d 轮计入统计" % ROUNDS,
            "db": "临时 SQLite（系统临时目录），结束时删除",
        },
        "groups": groups,
        "cache_detail": cache_detail,
        "cross_check": cross_check,
        "notes": notes,
    }
    return report


def sha256_file(path: str) -> Optional[str]:
    try:
        import hashlib

        h = hashlib.sha256()
        with open(path, "rb") as fh:
            for chunk in iter(lambda: fh.read(1 << 20), b""):
                h.update(chunk)
        return h.hexdigest()
    except Exception:  # noqa: BLE001
        return None


def environment_fingerprint() -> Dict[str, Any]:
    def _run(cmd: List[str]) -> Optional[str]:
        try:
            out = subprocess.run(
                cmd, capture_output=True, text=True, timeout=30, encoding="utf-8", errors="replace"
            )
            text = (out.stdout or out.stderr or "").strip()
            return text.splitlines()[0] if text else None
        except Exception:  # noqa: BLE001
            return None

    cpu = _run(["wmic", "cpu", "get", "name"]) or platform.processor() or "unknown"
    if cpu.lower().startswith("name"):
        cpu = cpu[4:].strip()
    return {
        "os": platform.platform(),
        "machine": platform.machine(),
        "cpu": cpu,
        "cpu_count_logical": os.cpu_count(),
        "rustc": _run(["rustc", "--version"]),
        "python": "Python %s" % platform.python_version(),
        "git_head": _run(["git", "-C", REPO_ROOT, "rev-parse", "--short", "HEAD"]),
    }


def cross_validate(groups: List[Dict[str, Any]], db_map: Dict[str, List[str]]) -> Dict[str, Any]:
    """直读 SQLite `voice_latency_samples`，按 route_kind 重算 P50/P95 与 HTTP 侧对比。

    HTTP 侧用**全轮次**样本（含被丢弃的首轮），保证与 DB 行集一一对应；
    报告里各组的 samples_ms 仍是“去掉首轮”后的口径。
    """
    import sqlite3

    out: Dict[str, Any] = {}
    for group in groups:
        name = group["group"]
        dbs = db_map.get(name, [])
        db_values: List[float] = []
        rows_by_kind: Dict[str, int] = {}
        missing_dbs: List[str] = []
        for db in dbs:
            if not os.path.exists(db):
                missing_dbs.append(db)
                continue
            try:
                conn = sqlite3.connect(db)
                try:
                    cur = conn.execute(
                        "SELECT route_kind, latency_ms FROM voice_latency_samples "
                        "WHERE route_kind = ?",
                        (name,),
                    )
                    for kind, latency in cur.fetchall():
                        db_values.append(float(latency))
                        rows_by_kind[kind] = rows_by_kind.get(kind, 0) + 1
                finally:
                    conn.close()
            except Exception as exc:  # noqa: BLE001
                missing_dbs.append("%s (%s)" % (db, exc))

        http_values = group.get("all_rounds_ms", [])
        http_stats = stats_block(http_values)
        db_stats = stats_block(db_values)
        entry: Dict[str, Any] = {
            "http_all_rounds": http_stats,
            "db_samples": db_stats,
            "db_rows_by_route_kind": rows_by_kind,
            "http_requests_all_rounds": len(http_values),
            "db_rows_total": len(db_values),
            "dbs": [os.path.basename(p) for p in dbs],
            "missing_dbs": missing_dbs,
        }
        diffs: Dict[str, Optional[float]] = {}
        flagged = False
        for key in ("p50_ms", "p95_ms", "p99_ms"):
            hv = http_stats.get(key)
            dv = db_stats.get(key)
            if hv is None or dv is None:
                diffs[key] = None
                continue
            if hv == 0:
                diffs[key] = 0.0 if dv == 0 else None
                continue
            pct = round((dv - hv) / hv * 100.0, 2)
            diffs[key] = pct
            if abs(pct) > 5.0:
                flagged = True
        entry["diff_pct_db_vs_http"] = diffs
        entry["flagged"] = flagged
        if flagged:
            entry["flag_reason"] = (
                "DB 侧 latency_ms 是**内核内部路由耗时**（整数毫秒，不含 HTTP 层），"
                "HTTP 侧是**完整往返 wall-clock**（TCP + axum 路由 + JSON 反序列化 + 响应序列化）。"
                "快路由本身在 0–3ms 量级，HTTP 固定开销占比因此很高，"
                "两者相差 > 5% 属预期，非采样错误。"
            )
        out[name] = entry
    return out


def print_summary(report: Dict[str, Any]) -> None:
    print("\n" + "=" * 78)
    print("路由延迟实测（HTTP 往返 wall-clock，首轮丢弃）")
    print("=" * 78)
    header = "%-10s %6s %9s %9s %9s %9s %9s %6s" % (
        "group",
        "n",
        "p50(ms)",
        "p95(ms)",
        "p99(ms)",
        "max(ms)",
        "min(ms)",
        "达标",
    )
    print(header)
    print("-" * 78)
    for g in report["groups"]:
        if not g.get("count"):
            print("%-10s %6d %9s" % (g["group"], 0, "N/A"))
            continue
        print(
            "%-10s %6d %9.2f %9.2f %9.2f %9.2f %9.2f %6s"
            % (
                g["group"],
                g["count"],
                g["p50_ms"],
                g["p95_ms"],
                g["p99_ms"],
                g["max_ms"],
                g["min_ms"],
                "OK" if g["target_met"] else "FAIL",
            )
        )
    print("-" * 78)
    for g in report["groups"]:
        if g.get("excluded"):
            print("[WARN] %s 组剔除 %d 条：%s" % (g["group"], len(g["excluded"]), g["excluded"]))
        if g.get("anomalies"):
            print("[WARN] %s 组异常样本 %d 条" % (g["group"], len(g["anomalies"])))
    print("\n交叉校验（DB voice_latency_samples vs HTTP 全轮次）：")
    for name, cc in report["cross_check"].items():
        print(
            "  %-10s http p50/p95=%s/%s  db p50/p95=%s/%s  diff=%s  %s"
            % (
                name,
                cc["http_all_rounds"].get("p50_ms"),
                cc["http_all_rounds"].get("p95_ms"),
                cc["db_samples"].get("p50_ms"),
                cc["db_samples"].get("p95_ms"),
                cc["diff_pct_db_vs_http"],
                "FLAGGED(>5%)" if cc["flagged"] else "ok",
            )
        )
    cd = report.get("cache_detail") or {}
    if cd:
        print(
            "\n缓存组：%d/%d 次命中（hit_rate=%.2f%%），stub 调用 %s 次（预热期预期 %s 次）"
            % (
                cd.get("hit_requests", 0),
                cd.get("total_requests", 0),
                (cd.get("hit_rate") or 0) * 100,
                cd.get("stub_calls_in_cache_phase"),
                cd.get("expected_stub_calls_in_cache_phase"),
            )
        )


def main() -> int:
    parser = argparse.ArgumentParser(description="VoicePilot 路由延迟基准（真实实测）")
    parser.add_argument("--out", default=None, help="输出 JSON 路径（默认 evals/reports/bench-<UTC>.json）")
    args = parser.parse_args()

    if not os.path.exists(BINARY):
        print(
            "找不到 %s\n先执行：cd voicepilot && cargo build -p cli" % BINARY,
            file=sys.stderr,
        )
        return 2

    tmp_root = tempfile.mkdtemp(prefix="vp-bench-")
    try:
        report = run_benchmark(tmp_root)
        ts = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
        out_path = args.out or os.path.join(REPORTS_DIR, "bench-%s.json" % ts)
        os.makedirs(os.path.dirname(os.path.abspath(out_path)), exist_ok=True)
        with open(out_path, "w", encoding="utf-8") as fh:
            json.dump(report, fh, ensure_ascii=False, indent=2)
        print_summary(report)
        print("\n报告已写入：%s" % out_path)
        return 0
    finally:
        _kill_all()
        shutil.rmtree(tmp_root, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
