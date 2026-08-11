"""VoicePilot 评测 scorers 包(W11 Plan 1)。

3 个 scorer:
- risk_level_scorer: 校验 risk_level 分级正确(L0-L3)
- undo_success_scorer: 校验 strong Compensation 回滚成功
- audit_completeness_scorer: 校验 OTel span 完整性 + audit_logs 覆盖率

W11 Plan 3 追加 toctou_block_scorer,W11 Plan 5 追加 egress_block_scorer。
"""
