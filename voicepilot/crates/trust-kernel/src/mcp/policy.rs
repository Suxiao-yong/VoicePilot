//! MCP 工具调用的策略闭环 —— 把外部 MCP 工具接进统一的 `ActionGateway::decide`。
//!
//! 设计契约见 `docs/MCP_GATEWAY_WEB_SERVICE_CONTRACT.md` §1。
//!
//! 三条不可调换的判定：
//!   1. annotation 一致性（`verify_mcp_annotations`）—— 谎报只读直接拦截；
//!   2. taint 硬拒（`check_taint_policy`，sink = `ToolArgument`）—— 不可信网页内容
//!      不能升级成工具指令；
//!   3. Cedar 授权 + E×D 风险矩阵（`Gateway::decide`）—— D3 凭据红线在网关内部。
//!
//! 注解（`readOnlyHint` / `destructiveHint` / `openWorldHint`）是外部 server 的
//! **不可信提示**：只允许把风险抬高，不允许降低。映射规则固定在本文件，
//! 由 `build_policy_request` 单测锁定。

use crate::error::{KernelError, Result};
use crate::mcp::repo::McpServerRecord;
use crate::policy::types::{DLevel, Decision, ELevel, EgressDest, Effect, Resource};

/// 统一 Cedar action 名。所有外部 MCP 工具都走这一条策略，工具身份放进
/// `resource.path = mcp://{server_id}/{tool_name}`，避免每个工具都要新增 policy。
pub const MCP_TOOL_CALL_ACTION: &str = "mcp_tool_call";

/// 一次 MCP 调用的策略评估输入（纯数据，便于单测）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpPolicyRequest {
    pub server_id: String,
    pub tool_name: String,
    pub e_level: ELevel,
    pub data_class: DLevel,
    pub egress: Option<EgressDest>,
    pub provenance: String,
}

impl McpPolicyRequest {
    /// Cedar resource：path 承载工具身份，data_class / provenance 承载数据画像。
    pub fn resource(&self) -> Resource {
        Resource {
            path: format!("mcp://{}/{}", self.server_id, self.tool_name),
            data_class: self.data_class,
            provenance: self.provenance.clone(),
        }
    }
}

/// 注解 → 策略输入（冻结映射，见契约 §1.2）。
///
/// - `E`：destructive → E2；否则 openWorld → E3；否则 readOnly → E0；否则 E1
/// - `D`：参数携带凭据形态 → D3；否则 readOnly 且无网络出站 → D0；否则 D1
/// - `egress`：openWorld（网络出站）→ RemoteMcp，否则 None（本地 stdio 无出站）
pub fn build_policy_request(
    server: &McpServerRecord,
    tool: &serde_json::Value,
    args: &serde_json::Value,
) -> McpPolicyRequest {
    let read_only = bool_hint(tool, "readOnlyHint");
    let destructive = bool_hint(tool, "destructiveHint");
    let open_world = bool_hint(tool, "openWorldHint");

    let e_level = if destructive {
        ELevel::E2
    } else if open_world {
        ELevel::E3
    } else if read_only {
        ELevel::E0
    } else {
        ELevel::E1
    };

    let data_class = if args_carry_credentials(args) {
        DLevel::D3
    } else if read_only && !open_world {
        DLevel::D0
    } else {
        DLevel::D1
    };

    McpPolicyRequest {
        server_id: server.server_id.clone(),
        tool_name: tool
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        e_level,
        data_class,
        egress: open_world.then_some(EgressDest::RemoteMcp),
        provenance: "user_direct".to_string(),
    }
}

fn bool_hint(tool: &serde_json::Value, name: &str) -> bool {
    tool.pointer(&format!("/annotations/{name}"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// 参数里是否出现凭据形态：key 命中 `is_credential_env_key`，或字符串值形如
/// `password=...` / `Authorization: ...`。只看结构，不落原文。
fn args_carry_credentials(args: &serde_json::Value) -> bool {
    match args {
        serde_json::Value::Object(map) => map.iter().any(|(k, v)| {
            crate::skills::common::is_credential_env_key(k)
                || k.eq_ignore_ascii_case("authorization")
                || args_carry_credentials(v)
        }),
        serde_json::Value::Array(items) => items.iter().any(args_carry_credentials),
        serde_json::Value::String(s) => looks_like_credential(s),
        _ => false,
    }
}

fn looks_like_credential(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    let marker = [
        "password",
        "passwd",
        "api_key",
        "api-key",
        "secret",
        "authorization",
        "bearer",
    ]
    .iter()
    .any(|marker| lower.contains(marker));
    marker && (lower.contains('=') || lower.contains(':') || lower.starts_with("bearer"))
}

/// 完整判定：annotation 校验 → taint 硬拒 → Cedar/E×D → 审计。
///
/// 返回 `Ok(Decision)` 仅当 `effect == Allow`；`Confirm` / `Deny` 都转成
/// `KernelError::PolicyDenied`（MCP 路径没有审批通道，fail-closed）。
pub fn evaluate_mcp_call(
    kernel: &crate::kernel::TrustKernel,
    task_id: &str,
    step_id: Option<&str>,
    server: &McpServerRecord,
    tool: &serde_json::Value,
    args: &serde_json::Value,
) -> Result<Decision> {
    // 1. annotation 一致性：谎报只读的写工具在这里止步。
    if let Err(e) = crate::mcp::schema::verify_mcp_annotations(tool) {
        let _ = kernel.audit_append_external(
            task_id,
            step_id,
            "malicious_server_detected",
            serde_json::json!({
                "server_id": server.server_id,
                "tool_name": tool.get("name").and_then(|v| v.as_str()).unwrap_or_default(),
                "reason": e.to_string(),
            }),
        );
        return Err(e);
    }

    let mut request = build_policy_request(server, tool, args);

    // 2. taint：参数 hash 查 TaintRepo，web_page → ToolArgument 硬拒。
    //    先取 taints 用于 provenance，再交给带审计的检查函数（两次短锁，无嵌套）。
    let args_hash = crate::policy::taint_repo::compute_value_hash(args);
    let taints: Vec<String> = {
        let conn = kernel.conn();
        crate::policy::taint_repo::TaintRepo::new()
            .find_by_hash(&conn, &args_hash)?
            .map(|r| r.taints)
            .unwrap_or_default()
    };
    if let Some(first) = taints.first() {
        request.provenance = first.clone();
    }
    crate::gateway::check_taint_policy_and_audit(
        kernel,
        task_id,
        step_id,
        &args_hash,
        EgressDest::ToolArgument,
    )?;

    // 3. Cedar 授权 + E×D 矩阵 + egress。
    let decision = kernel.gateway().decide(
        MCP_TOOL_CALL_ACTION,
        request.e_level,
        &request.resource(),
        request.egress,
        Some(args.clone()),
    )?;

    // 4. 审计裁决结果（details 只含标签与策略元数据，不含参数原文）。
    let audit_details = serde_json::json!({
        "server_id": request.server_id,
        "tool_name": request.tool_name,
        "effect": format!("{:?}", decision.effect).to_lowercase(),
        "e_level": request.e_level.as_str(),
        "data_class": request.data_class.as_str(),
        "egress": request.egress.map(|e| e.as_str()),
        "matched_policies": decision.matched_policies,
        "bundle_hash": decision.policy_bundle_hash,
        "reasons": decision.reasons,
        "args_hash": args_hash,
    });
    kernel.audit_append_external(task_id, step_id, "mcp_policy_decision", audit_details)?;

    match decision.effect {
        Effect::Allow => Ok(decision),
        Effect::Confirm => Err(KernelError::PolicyDenied(format!(
            "MCP tool '{}' on server '{}' requires approval (E{}×D{}); MCP dispatch has no approval channel",
            request.tool_name,
            request.server_id,
            request.e_level.as_str(),
            request.data_class.as_str()
        ))),
        Effect::Deny => Err(KernelError::PolicyDenied(format!(
            "MCP tool '{}' on server '{}' denied (E{}×D{}): {}",
            request.tool_name,
            request.server_id,
            request.e_level.as_str(),
            request.data_class.as_str(),
            decision.reasons.join("; ")
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> McpServerRecord {
        McpServerRecord {
            server_id: "playwright".into(),
            name: "playwright".into(),
            version: "1".into(),
            transport: "stdio".into(),
            enabled: true,
            trusted: true,
            protocol_version: None,
            allowed_origins: None,
            allowed_paths: None,
            command: Some("npx".into()),
            args: None,
            env: None,
        }
    }

    fn tool(name: &str, read_only: bool, destructive: bool, open_world: bool) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "annotations": {
                "readOnlyHint": read_only,
                "destructiveHint": destructive,
                "openWorldHint": open_world,
            }
        })
    }

    #[test]
    fn readonly_local_tool_maps_to_e0_d0_without_egress() {
        let req = build_policy_request(
            &server(),
            &tool("snapshot", true, false, false),
            &serde_json::json!({"url": "https://example.com"}),
        );
        assert_eq!(req.e_level, ELevel::E0);
        assert_eq!(req.data_class, DLevel::D0);
        assert_eq!(req.egress, None);
        assert_eq!(req.resource().path, "mcp://playwright/snapshot");
    }

    #[test]
    fn open_world_tool_maps_to_e3_remote_mcp_egress() {
        let req = build_policy_request(
            &server(),
            &tool("browse", true, false, true),
            &serde_json::json!({}),
        );
        assert_eq!(req.e_level, ELevel::E3);
        assert_eq!(req.egress, Some(EgressDest::RemoteMcp));
    }

    #[test]
    fn destructive_tool_maps_to_e2() {
        let req = build_policy_request(
            &server(),
            &tool("purge", false, true, false),
            &serde_json::json!({}),
        );
        assert_eq!(req.e_level, ELevel::E2);
    }

    #[test]
    fn credential_shaped_args_map_to_d3() {
        let req = build_policy_request(
            &server(),
            &tool("login", false, false, false),
            &serde_json::json!({"api_key": "sk-live"}),
        );
        assert_eq!(req.data_class, DLevel::D3);

        let nested = build_policy_request(
            &server(),
            &tool("login", false, false, false),
            &serde_json::json!({"headers": {"Authorization": "Bearer abc"}}),
        );
        assert_eq!(nested.data_class, DLevel::D3);
    }
}
