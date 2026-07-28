//! RouterBridge — wires transcribed text → SkillRouter → Skill execution.
//!
//! V1.1 §2.1 + §5.1: voice input pipeline terminal stage.
//! Returns RouteOutcome so caller (CLI) can decide UI feedback.

use crate::approval::approver::Approver;
use crate::error::Result;
use crate::kernel::TrustKernel;
use crate::skills::manifest::{
    files_organize_manifest, form_prepare_manifest, research_save_manifest,
    task_compensate_manifest, task_explain_manifest, task_repeat_verified_manifest,
};
use crate::skills::router::{RouteDecision, SkillRouter};

#[derive(Debug)]
pub enum RouteOutcome {
    /// Skill matched. Caller (CLI) prompts user for args, then invokes
    /// `FilesOrganizeSkill::execute` to run the prepare→approve→commit flow.
    Routed {
        skill_id: String,
    },
    /// W8 Plan 4 新增:LLM 拆解为多步 DAG。Caller 应弹 DAG 骨架审批 UI
    /// (Plan 5 实现),Allow 后调 `DagExecutor::run` 执行。
    #[cfg(feature = "llm")]
    DagPlan(crate::skills::dag_types::DagPlan),
    /// No skill matched; caller should fall back to LLM Planner (W7).
    Unmatched { text: String },
    /// Input was empty/whitespace.
    Empty,
}

/// Route transcribed text through SkillRouter.
///
/// If a Skill is matched, this fn does NOT execute the Skill — execution
/// requires user-supplied arguments (source, filter, destination) that
/// aren't derivable from the voice text alone in W5. W5 PoC: caller (CLI)
/// prompts user for missing args. W7 LLM Planner will extract args from
/// text automatically.
///
/// The `_kernel` and `_approver` params are unused in W5 (routing only);
/// they exist so W7 can extend this fn to invoke the Skill executor
/// directly without changing the call signature.
pub fn route_text(
    _kernel: &TrustKernel,
    _approver: &dyn Approver,
    text: &str,
) -> Result<RouteOutcome> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(RouteOutcome::Empty);
    }

    let mut router = SkillRouter::new();
    router.register(files_organize_manifest());
    router.register(task_repeat_verified_manifest());
    // 注册顺序: task_compensate 必须在 task_explain 之前,否则 task_explain 的
    // keyword "上一步" 会先匹配 "撤销上一步" / "补偿上一步" 等 compensate 查询。
    router.register(task_compensate_manifest());
    router.register(task_explain_manifest());
    // W7 Plan 4: register UIA skills (Windows-only, opt-in via `uia` feature).
    #[cfg(all(windows, feature = "uia"))]
    {
        use crate::skills::manifest::{app_control_manifest, note_capture_manifest};
        router.register(app_control_manifest());
        router.register(note_capture_manifest());
    }
    // W7 Plan 5: register Playwright MCP browser skills (cross-platform,
    // no `uia` gate — they depend only on the MCP client, which runs on
    // any OS that can spawn `npx @playwright/mcp`).
    router.register(research_save_manifest());
    router.register(form_prepare_manifest());

    match router.route(trimmed) {
        RouteDecision::Skill(manifest) => Ok(RouteOutcome::Routed {
            skill_id: manifest.id,
        }),
        // 同步 `route()` 不调用 LLM,不会产生 SkillWithSlots;若上游契约被破坏,
        // 退化为 Planner 而非 panic,保持 voice pipeline 鲁棒性。
        #[cfg(feature = "llm")]
        RouteDecision::SkillWithSlots(manifest, _slots) => Ok(RouteOutcome::Routed {
            skill_id: manifest.id,
        }),
        // 同步 `route()` 不返回 Dag;此处 unreachable,保持 match 穷尽。
        #[cfg(feature = "llm")]
        RouteDecision::Dag(_) => Ok(RouteOutcome::Unmatched {
            text: trimmed.to_string(),
        }),
        RouteDecision::Planner => Ok(RouteOutcome::Unmatched {
            text: trimmed.to_string(),
        }),
    }
}

/// W8 Plan 4: 三级路由策略(spec §2.8)。
///
/// 1. **关键词优先**:`SkillRouter::route(text)` 同步命中 → 直接返回 `Routed`,
///    不调 LLM。匹配 W7 §2.2 算法:keyword / intent_example 命中即返回。
/// 2. **LLM 拆解**(W8 新):关键词未命中且 LLM 启用 + `!privacy_mode` 时,
///    调 `LlmClient::decompose_to_dag_traced` → `SlotTemplateEngine::validate_dag`
///    双层防御(spec §2.1 / §6)→ 返回 `DagPlan`。
///    任何错误(HTTP / 解析 / 校验)catch 后回退到第 3 级。
///    LLM 成功路径通过 `record_llm_decompose_called` 记录审计事件
///    (硬约束:`plan_id / llm_model / latency_ms / token_count` 4 字段)。
/// 3. **W7 回退**:调 `route_with_llm` 走 W7 关键词 + LLM 单 Skill fallback。
///
/// 与 W7 `route_text` 的区别:
/// - async(LLM 调用是 async)
/// - 不接收 `approver` 参数(本函数只路由不执行,DAG 执行时由 DagExecutor 自带 approver)
/// - 返回 `RouteOutcome` 而非 `RouteDecision`(便于调用方直接处理 DAG / Routed / Unmatched)
///
/// # Errors
/// - `KernelError::Db` 当 KV 读取 privacy_mode 失败时(保守策略:返回 Err 让上层处理)
/// - `KernelError::Db` 当 `create_task` 失败时(LLM 调用前需预创建 task 满足审计 FK)
/// - 其他错误均被 catch,回退到 W7 单 Skill 路由(返回 `route_with_llm` 结果)
#[cfg(feature = "voice")]
pub async fn route_text_with_dag(
    kernel: &TrustKernel,
    text: &str,
) -> crate::error::Result<RouteOutcome> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(RouteOutcome::Empty);
    }

    // 构建 SkillRouter,注册 W7 全部 built-in Skills(与 route_text 一致)。
    let mut router = SkillRouter::new();
    router.register(files_organize_manifest());
    router.register(task_repeat_verified_manifest());
    // 注册顺序:task_compensate 必须在 task_explain 之前(与 route_text 一致)。
    router.register(task_compensate_manifest());
    router.register(task_explain_manifest());
    #[cfg(all(windows, feature = "uia"))]
    {
        use crate::skills::manifest::{app_control_manifest, note_capture_manifest};
        router.register(app_control_manifest());
        router.register(note_capture_manifest());
    }
    router.register(research_save_manifest());
    router.register(form_prepare_manifest());

    // ===== 第 1 级:关键词优先(spec §2.8)=====
    // SkillRouter::route 是同步方法,只做 keyword + intent_example 匹配,不调 LLM。
    // 命中 Skill / SkillWithSlots → 直接返回 Routed,跳过 LLM 拆解。
    let keyword_decision = router.route(trimmed);
    match keyword_decision {
        RouteDecision::Skill(manifest) => {
            return Ok(RouteOutcome::Routed {
                skill_id: manifest.id,
            });
        }
        #[cfg(feature = "llm")]
        RouteDecision::SkillWithSlots(manifest, _slots) => {
            return Ok(RouteOutcome::Routed {
                skill_id: manifest.id,
            });
        }
        #[cfg(feature = "llm")]
        RouteDecision::Dag(_) => {
            // route() 是同步方法,从不返回 Dag;此处 unreachable,保持防御
        }
        RouteDecision::Planner => { /* 落到第 2 级 */ }
    }

    // ===== 第 2 级:LLM 拆解(spec §2.8)=====
    // 关键词未命中且 LLM 启用 + !privacy_mode 时,尝试拆解为多步 DAG。
    // 任何错误(HTTP / 解析 / 校验)catch 后回退到第 3 级 route_with_llm。
    #[cfg(feature = "llm")]
    {
        if let Some(llm) = kernel.llm_client() {
            if llm.is_enabled() && !kernel.privacy_mode() {
                // spec §6.1 审计事件:llm_decompose_called(硬约束 4 字段:
                // plan_id / llm_model / latency_ms / token_count)。
                // audit_logs.task_id NOT NULL + FK 约束要求先创建 task。
                // LLM 成功 / 失败均保留 task-llm-{uuid} 作为审计回溯依据
                // (DagExecutor::run 创建自己的 root_task_id,不复用此临时 task)。
                let llm_task_id = format!("task-llm-{}", uuid::Uuid::new_v4());
                kernel.create_task(&llm_task_id, &format!("LLM decompose for: {}", trimmed))?;

                // spec §2.2:decompose_to_dag_traced(text, candidate_skills, user_slots)
                // user_slots 暂传空 slice(W7 route_with_llm 也是 LLM 内部提取 slots,
                // 不预先 regex 解析);Plan 5+ 视需要补 slot_parser 模块。
                // 用 _traced 版本拿 DecomposeStats,再调 record_llm_decompose_called
                // 满足硬约束(plan_id / llm_model / latency_ms / token_count 全字段)。
                let user_slots: Vec<crate::llm::types::ExtractedSlot> = Vec::new();
                match llm
                    .decompose_to_dag_traced(trimmed, router.skills(), &user_slots)
                    .await
                {
                    Ok((dag, stats)) => {
                        // 审计 — llm_decompose_called(成功路径,spec §6.1)
                        let _ = crate::llm::client::record_llm_decompose_called(
                            kernel,
                            &llm_task_id,
                            &dag.plan_id,
                            &stats,
                        );

                        // 双层防御 #1(spec §2.1 / §6):LLM 返回后立即校验。
                        // validate_dag 检查:node_id 引用 / slot kind / iter 仅在循环节点 /
                        // filter predicate 支持。decompose_to_dag_traced 内部已校验,
                        // 此处二次校验作为 defense-in-depth(防止 Plan 2 实现遗漏)。
                        match crate::skills::template::SlotTemplateEngine::validate_dag(&dag) {
                            Ok(()) => {
                                return Ok(RouteOutcome::DagPlan(dag));
                            }
                            Err(e) => {
                                // 校验失败:回退到 W7 route_with_llm(spec §2.8 错误处理)。
                                tracing::warn!(
                                    error = %e,
                                    "validate_dag failed; falling back to W7 route_with_llm"
                                );
                                // 落到第 3 级(下方 route_with_llm 调用)
                            }
                        }
                    }
                    Err(e) => {
                        // decompose_to_dag 失败(HTTP / 解析 / 超时):回退到 W7 route_with_llm。
                        // 不发 llm_decompose_called 审计事件(LLM 调用未成功,
                        // tracing::warn! 已记录错误;审计链路在 DagExecutor::run 内
                        // 通过 dag_plan_created → dag_completed 闭合,LLM 失败路径
                        // 不进入 DagExecutor,无审计缺口)。
                        tracing::warn!(
                            error = ?e,
                            "decompose_to_dag failed; falling back to W7 route_with_llm"
                        );
                        // 落到第 3 级
                    }
                }
            }
        }
    }

    // ===== 第 3 级:W7 route_with_llm 回退(spec §2.8)=====
    // LLM 拆解失败 / 校验失败 / LLM disabled / privacy_mode=true 时,走 W7 单 Skill 路由。
    // route_with_llm 内部:keyword 匹配(已在第 1 级做过,此处冗余但 W7 逻辑保持)→
    // LLM classify_and_extract → Planner。
    //
    // 需要 LLM 启用的 router:若 kernel.llm_client() 为 Some,用 with_llm 构造新 router;
    // 否则用第 1 级的 keyword-only router(此时 route_with_llm 退化为 route,返回 Planner)。
    #[cfg(feature = "llm")]
    {
        let llm_router = build_router_with_llm(kernel, &router);
        let decision = llm_router.route_with_llm(trimmed).await;
        return Ok(decision_to_outcome(decision, trimmed));
    }

    // 无 llm feature 时:第 1 级 keyword 未命中 → 直接 Unmatched
    #[allow(unreachable_code)]
    Ok(RouteOutcome::Unmatched {
        text: trimmed.to_string(),
    })
}

/// 用 kernel.llm_client() 构造带 LLM 的 SkillRouter(若 LLM 不可用则用原 router)。
/// route_with_llm 是 SkillRouter 方法,需 router 持有 LlmClient。
#[cfg(all(feature = "voice", feature = "llm"))]
fn build_router_with_llm(
    kernel: &TrustKernel,
    keyword_router: &SkillRouter,
) -> SkillRouter {
    if let Some(llm) = kernel.llm_client() {
        // 用 with_llm 构造新 router,重新注册全部 Skills(与 keyword_router 一致)。
        let mut new_router = SkillRouter::with_llm(llm);
        for skill in keyword_router.skills() {
            new_router.register(skill.clone());
        }
        new_router
    } else {
        // LLM 不可用:用原 keyword-only router 的 clone(SkillRouter 是 Clone)。
        // route_with_llm 在 LLM=None 时退化为 keyword 匹配,行为与 route() 一致。
        keyword_router.clone()
    }
}

/// RouteDecision → RouteOutcome 转换。
#[cfg(feature = "voice")]
fn decision_to_outcome(decision: RouteDecision, text: &str) -> RouteOutcome {
    match decision {
        RouteDecision::Skill(manifest) => RouteOutcome::Routed {
            skill_id: manifest.id,
        },
        #[cfg(feature = "llm")]
        RouteDecision::SkillWithSlots(manifest, _slots) => RouteOutcome::Routed {
            skill_id: manifest.id,
        },
        #[cfg(feature = "llm")]
        RouteDecision::Dag(plan) => {
            // route_with_llm 不返回 Dag(它调 classify_and_extract 不是 decompose_to_dag);
            // 此 arm 防御性:若上游契约被破坏,把 Dag 转为 DagPlan 暴露给调用方。
            RouteOutcome::DagPlan(plan)
        }
        RouteDecision::Planner => RouteOutcome::Unmatched {
            text: text.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::approver::AutoApprover;
    use crate::kernel::TrustKernel;

    /// W7 Plan 4 Task 6: `quick.app_control` 注册后,`route_text` 应将
    /// "打开记事本" 路由到该 Skill(intent_example 命中)。仅在 `uia` feature
    /// 开启时验证 —— 无 uia 时 manifest 未注册,路由会落到 Planner。
    #[cfg(all(windows, feature = "uia"))]
    #[test]
    fn route_text_recognizes_app_control_intent() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let result = route_text(&kernel, &approver, "打开记事本").unwrap();
        assert!(
            matches!(result, RouteOutcome::Routed { ref skill_id } if skill_id == "quick.app_control"),
            "expected Routed to quick.app_control, got {:?}",
            result
        );
    }

    /// W7 Plan 4 Task 6: `note.capture` 注册后,`route_text` 应将
    /// "用记事本记录这个想法" 路由到该 Skill(intent_example "用记事本记录" 命中)。
    #[cfg(all(windows, feature = "uia"))]
    #[test]
    fn route_text_recognizes_note_capture_intent() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let result = route_text(&kernel, &approver, "用记事本记录这个想法").unwrap();
        assert!(
            matches!(result, RouteOutcome::Routed { ref skill_id } if skill_id == "note.capture"),
            "expected Routed to note.capture, got {:?}",
            result
        );
    }

    /// W7 Plan 5 Task 6: `research.save_markdown` 注册后,`route_text` 应将
    /// "把这个网页存为 Markdown" 路由到该 Skill(keyword "网页" + "Markdown" 命中)。
    /// 跨平台,无 cfg 门控。
    #[test]
    fn route_text_recognizes_research_save_intent() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let result = route_text(&kernel, &approver, "把这个网页存为 Markdown").unwrap();
        assert!(
            matches!(result, RouteOutcome::Routed { ref skill_id } if skill_id == "research.save_markdown"),
            "expected Routed to research.save_markdown, got {:?}",
            result
        );
    }

    /// W7 Plan 5 Task 6: `form.prepare` 注册后,`route_text` 应将
    /// "帮我填这个表单" 路由到该 Skill(keyword "表单" + "填" 命中)。跨平台。
    #[test]
    fn route_text_recognizes_form_prepare_intent() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let result = route_text(&kernel, &approver, "帮我填这个表单").unwrap();
        assert!(
            matches!(result, RouteOutcome::Routed { ref skill_id } if skill_id == "form.prepare"),
            "expected Routed to form.prepare, got {:?}",
            result
        );
    }
}
