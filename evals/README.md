# VoicePilot 评测基础设施(W11)

> **WIP:** W11 Plan 1 进行中,完整说明将在 Plan 1 完成时补全。

## 目录结构

```
evals/
├── functional/         # 100 功能任务(Inspect AI)
├── redteam/            # 50 攻击样本(promptfoo red team)
├── toctou/             # 20 TOCTOU 场景(Rust 单测)
├── malicious_server/   # 15 恶意 MCP Server 场景(Rust 单测)
├── data_security/      # 20 数据安全场景(promptfoo)
├── scorers/            # Python scorer
└── run_all.sh          # 串联脚本(W11 Plan 6)
```

## 运行(待 Plan 6 完成)

```powershell
cd d:\voicepilot\evals
python -m venv .venv
.\.venv\Scripts\Activate.ps1
pip install -e .
inspect eval inspect_evals.py
```
