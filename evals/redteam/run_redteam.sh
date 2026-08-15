#!/usr/bin/env bash
# VoicePilot V1 评测 — red team 50 攻击样本运行脚本(W11 Plan 2,spec §9.4 ②)
#
# 运行:
#   cd d:\voicepilot\evals
#   bash redteam/run_redteam.sh
#
# 前置:voicepilot CLI 已构建(target/debug/voicepilot.exe)+ promptfoo 已安装
# 输出:evals/reports/<timestamp>/redteam.json
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
EVALS_DIR="${REPO_ROOT}/evals"
TIMESTAMP=$(date -u +"%Y%m%dT%H%M%SZ")
REPORT_DIR="${EVALS_DIR}/reports/${TIMESTAMP}"
mkdir -p "${REPORT_DIR}"

# 确保 CLI 已构建
if [ ! -f "${REPO_ROOT}/voicepilot/target/debug/voicepilot.exe" ]; then
  echo "ERROR: voicepilot.exe not found. Build first:"
  echo "  cd ${REPO_ROOT} && cargo build --manifest-path voicepilot/Cargo.toml -p cli --no-default-features"
  exit 1
fi

echo "==> Running red team eval (50 attacks) ..."
promptfoo eval -c "${EVALS_DIR}/redteam/promptfooconfig.yaml" \
  --output "${REPORT_DIR}/redteam.json" || { echo "red team eval FAILED"; exit 1; }

echo "==> Red team report: ${REPORT_DIR}/redteam.json"
