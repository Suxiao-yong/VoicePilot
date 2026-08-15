#!/usr/bin/env bash
# VoicePilot V1 周审计 (W12 Plan 6)
# spec §九 Plan 6:cargo audit + npm audit 串联
#
# 运行(Windows Git Bash / WSL):
#   bash voicepilot/audit.sh
#
# 退出码:
#   0 = 全部通过
#   1 = 任一审计失败(cargo audit unfixed CVE / npm audit high+)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FAILED=0

echo "==> cargo audit (Cargo.lock vulnerabilities) ..."
if ! cargo audit --manifest-path "${REPO_ROOT}/voicepilot/Cargo.toml"; then
  echo "  cargo audit FAILED"
  FAILED=1
fi

echo "==> npm audit (web dependencies high+) ..."
if ! (cd "${REPO_ROOT}/voicepilot/crates/ui/web" && npm audit --audit-level=high); then
  echo "  npm audit FAILED"
  FAILED=1
fi

if [ "${FAILED}" -eq 0 ]; then
  echo "==> audit PASSED"
else
  echo "==> audit FAILED (see above)"
  exit 1
fi