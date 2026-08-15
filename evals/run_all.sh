#!/usr/bin/env bash
# VoicePilot V1 评测 — 串联 5 项评测(spec §9.4 ①-⑤),生成 V1 门禁 summary。
#
# 运行(开发机本地,Windows Git Bash / WSL):
#   cd d:\voicepilot
#   bash evals/run_all.sh
#
# 输出:evals/reports/<timestamp>/summary.json,v1_gate_verdict = PASS|FAIL
#
# 前置:
#   - cargo build -p cli --no-default-features(voicepilot/target/debug/voicepilot.exe)
#   - Python venv + inspect-ai + promptfoo(见 evals/README.md)
#   - Rust smoke 测试随 cargo test 运行(toctou / malicious_server / data_security)
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
EVALS_DIR="${REPO_ROOT}/evals"
TIMESTAMP=$(date -u +"%Y%m%dT%H%M%SZ")
REPORT_DIR="${EVALS_DIR}/reports/${TIMESTAMP}"
mkdir -p "${REPORT_DIR}"

echo "=================================================="
echo " VoicePilot V1 eval run @ ${TIMESTAMP}"
echo " report dir: ${REPORT_DIR}"
echo "=================================================="

# ① 100 功能任务(Inspect AI)
echo "==> ① functional (100 tasks, Inspect AI) ..."
inspect eval "${EVALS_DIR}/inspect_evals.py" --output "${REPORT_DIR}/functional.json" \
  || { echo "  functional eval FAILED"; }
FUNC_PASS=$(python - <<'PY'
import json, glob
files = glob.glob("reports/*/functional.json") + glob.glob("reports/*/functional-*.json")
print(len(files))
PY
)
# ② 50 攻击样本(promptfoo red team)
echo "==> ② redteam (50 attacks, promptfoo) ..."
promptfoo eval -c "${EVALS_DIR}/redteam/promptfooconfig.yaml" \
  --output "${REPORT_DIR}/redteam.json" \
  || { echo "  redteam eval FAILED"; }

# ③ 20 TOCTOU(Rust smoke)
echo "==> ③ toctou (20 scenarios, cargo test) ..."
(cd "${REPO_ROOT}" && cargo test --manifest-path voicepilot/Cargo.toml \
  -p trust-kernel --test w11_toctou_block_smoke -- --nocapture) \
  > "${REPORT_DIR}/toctou.txt" || { echo "  toctou smoke FAILED"; }

# ④ 15 恶意 Server(Rust smoke)
echo "==> ④ malicious_server (15 scenarios, cargo test) ..."
(cd "${REPO_ROOT}" && cargo test --manifest-path voicepilot/Cargo.toml \
  -p trust-kernel --test w11_malicious_server_smoke -- --nocapture) \
  > "${REPORT_DIR}/malicious_server.txt" || { echo "  malicious_server smoke FAILED"; }

# ⑤ 20 数据安全(Rust smoke)
echo "==> ⑤ data_security (20 scenarios, cargo test) ..."
(cd "${REPO_ROOT}" && cargo test --manifest-path voicepilot/Cargo.toml \
  -p trust-kernel --test w11_data_security_smoke -- --nocapture) \
  > "${REPORT_DIR}/data_security.txt" || { echo "  data_security smoke FAILED"; }

# 汇总
echo "==> summarizing ..."
python "${EVALS_DIR}/summarize.py" "${REPORT_DIR}" > "${REPORT_DIR}/summary.json"
cat "${REPORT_DIR}/summary.json"
echo ""
echo "==> done. summary: ${REPORT_DIR}/summary.json"
