//! LLM 客户端 — W7 §2.1.
//!
//! OpenAI 兼容 `/chat/completions` + function calling 强制结构化输出。
//! 失败时返回 `LlmError`,SkillRouter 回退到关键词匹配。

use std::time::Duration;

use reqwest::Client;
use serde_json::json;

use crate::llm::types::{ExtractedSlot, LlmError, LlmResult, LlmRouteResponse};
use crate::skills::manifest::SkillManifest;

const DEFAULT_TIMEOUT_SECS: u64 = 30;

pub struct LlmClient {
    base_url: String,
    api_key: String,
    model: String,
    http: Client,
    timeout: Duration,
}

impl std::fmt::Debug for LlmClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmClient")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("timeout", &self.timeout)
            .field("enabled", &self.is_enabled())
            .finish_non_exhaustive()
    }
}

impl LlmClient {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Self {
        let timeout = Duration::from_secs(DEFAULT_TIMEOUT_SECS);
        let http = Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            http,
            timeout,
        }
    }

    /// 返回未配置的 no-op 客户端(api_key 为空或 privacy_mode = true 时用)
    pub fn disabled() -> Self {
        Self::new("", "", "")
    }

    pub fn is_enabled(&self) -> bool {
        !self.api_key.is_empty() && !self.base_url.is_empty()
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// 意图分类 + Slot 提取(单次 LLM 调用)。
    pub async fn classify_and_extract(
        &self,
        text: &str,
        candidate_skills: &[SkillManifest],
    ) -> LlmResult<LlmRouteResponse> {
        if !self.is_enabled() {
            return Err(LlmError::NotConfigured);
        }

        let system_prompt = self.build_system_prompt(candidate_skills);
        let tools = self.build_tool_schema();
        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": text},
            ],
            "tools": tools,
            "tool_choice": {"type": "function", "function": {"name": "route_skill"}},
            "temperature": 0.1,
        });

        let url = format!("{}/chat/completions", self.base_url);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    LlmError::Timeout(self.timeout)
                } else {
                    LlmError::Http(e.to_string())
                }
            })?;

        if !resp.status().is_success() {
            return Err(LlmError::Http(format!("HTTP {}", resp.status())));
        }

        let resp_json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| LlmError::Parse(format!("response body parse: {e}")))?;

        self.parse_tool_call_response(&resp_json)
    }

    fn build_system_prompt(&self, skills: &[SkillManifest]) -> String {
        let mut s = String::from(
            "你是 VoicePilot 的意图分类器,从用户语音转写文本中识别要执行的 Skill。\n\n候选 Skill 列表:\n",
        );
        for skill in skills {
            let input_keys: Vec<&String> = skill.inputs.keys().collect();
            s.push_str(&format!(
                "- id: {}\n  title: {}\n  description: {}\n  intent_examples: {:?}\n  inputs: {:?}\n\n",
                skill.id, skill.title, skill.description, skill.intent_examples, input_keys
            ));
        }
        s.push_str(
            "\n若没有匹配的 Skill,返回 matched_skill_id=null + confidence<0.7。\nSlot 提取遵循 inputs 中的 input_type 约束。\n不得执行任何动作,只返回路由决策。",
        );
        s
    }

    fn build_tool_schema(&self) -> serde_json::Value {
        json!([{
            "type": "function",
            "function": {
                "name": "route_skill",
                "description": "Route user text to a skill and extract slots",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "matched_skill_id": {"type": ["string", "null"]},
                        "confidence": {"type": "number", "minimum": 0, "maximum": 1},
                        "slots": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "kind": {"type": "string"},
                                    "raw": {"type": "string"},
                                    "high_risk": {"type": "boolean"}
                                },
                                "required": ["kind", "raw", "high_risk"]
                            }
                        },
                        "reasoning": {"type": "string"}
                    },
                    "required": ["matched_skill_id", "confidence", "slots", "reasoning"]
                }
            }
        }])
    }

    fn parse_tool_call_response(&self, resp: &serde_json::Value) -> LlmResult<LlmRouteResponse> {
        let tool_call = resp
            .pointer("/choices/0/message/tool_calls/0")
            .ok_or_else(|| LlmError::Parse("missing tool_calls[0]".to_string()))?;
        let args_str = tool_call
            .pointer("/function/arguments")
            .and_then(|v| v.as_str())
            .ok_or_else(|| LlmError::Parse("missing function.arguments".to_string()))?;
        let args: serde_json::Value = serde_json::from_str(args_str)
            .map_err(|e| LlmError::Parse(format!("arguments parse: {e}")))?;

        let matched_skill_id = args
            .get("matched_skill_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let confidence = args
            .get("confidence")
            .and_then(|v| v.as_f64())
            .map(|f| f as f32)
            .unwrap_or(0.0);
        let reasoning = args
            .get("reasoning")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let slots = args
            .get("slots")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|s| {
                        Some(ExtractedSlot {
                            kind: s.get("kind")?.as_str()?.to_string(),
                            raw: s.get("raw")?.as_str()?.to_string(),
                            high_risk: s.get("high_risk")?.as_bool()?,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(LlmRouteResponse {
            matched_skill_id,
            confidence,
            slots,
            reasoning,
        })
    }

    /// W8 Plan 2 Task 8:带自定义 timeout 的构造器(主要供 wiremock 测试用,
    /// 让 timeout 测试能在 1s 内完成而非等满 30s 默认值)。
    ///
    /// 生产代码用 `LlmClient::new`(30s 默认)。本方法 `pub` 暴露给集成测试。
    #[cfg(feature = "llm")]
    pub fn with_timeout(base_url: &str, api_key: &str, model: &str, timeout: Duration) -> Self {
        let http = Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            http,
            timeout,
        }
    }

    /// W8 Plan 2:语音 → 完整 DAG plan。
    ///
    /// 复用 W7 的 OpenAI 兼容 `/chat/completions` + function calling。
    /// 失败时返回 `LlmError`,调用方(router_bridge Plan 4)catch 并回退到
    /// W7 单 Skill 关键词路由。
    ///
    /// 校验(双层防御 Layer 1,spec §2.1):
    /// 1. max_total_steps ≤ 20
    /// 2. 所有 skill_id 在 candidate_skills 中
    /// 3. SlotTemplateEngine::validate_dag 通过(模板语法 + 引用合法性)
    /// 4. DagPlan::validate_edges + validate_loop_specs 通过
    ///
    /// 任一校验失败 → Err(LlmError::Parse),不返回部分结果。
    ///
    /// 注:本方法不记录 `llm_decompose_called` 审计(LlmClient 不持有
    /// TrustKernel 引用)。调用方用 `decompose_to_dag_traced` 取得
    /// `DecomposeStats`,再调 `record_llm_decompose_called` 写审计。
    #[cfg(feature = "llm")]
    pub async fn decompose_to_dag(
        &self,
        user_text: &str,
        candidate_skills: &[SkillManifest],
        user_slots: &[ExtractedSlot],
    ) -> LlmResult<crate::skills::dag_types::DagPlan> {
        let (plan, _stats) = self.decompose_to_dag_traced(user_text, candidate_skills, user_slots).await?;
        Ok(plan)
    }

    /// W8 Plan 2 Task 8:`decompose_to_dag` 的 traced 版本,返回 plan + `DecomposeStats`。
    ///
    /// `DecomposeStats` 携带 `llm_model / latency_ms / token_count`,调用方
    /// (router_bridge Plan 4)用 `record_llm_decompose_called` 把这些字段
    /// 连同 `plan_id` 一起记入 `llm_decompose_called` 审计事件(硬约束)。
    #[cfg(feature = "llm")]
    pub async fn decompose_to_dag_traced(
        &self,
        user_text: &str,
        candidate_skills: &[SkillManifest],
        user_slots: &[ExtractedSlot],
    ) -> LlmResult<(crate::skills::dag_types::DagPlan, crate::llm::types::DecomposeStats)> {
        use crate::skills::dag_types::MAX_TOTAL_STEPS_HARD_LIMIT;
        use crate::skills::template::SlotTemplateEngine;
        use std::time::Instant;

        if !self.is_enabled() {
            return Err(LlmError::NotConfigured);
        }

        let system_prompt = self.build_decompose_system_prompt(candidate_skills, user_slots);
        let tools = self.build_decompose_tool_schema();
        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_text},
            ],
            "tools": tools,
            "tool_choice": {"type": "function", "function": {"name": "decompose_to_dag"}},
            "temperature": 0.1,
        });

        let url = format!("{}/chat/completions", self.base_url);
        let started = Instant::now();
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    LlmError::Timeout(self.timeout)
                } else {
                    LlmError::Http(e.to_string())
                }
            })?;
        let latency_ms = started.elapsed().as_millis() as u64;

        if !resp.status().is_success() {
            return Err(LlmError::Http(format!("HTTP {}", resp.status())));
        }

        let resp_json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| LlmError::Parse(format!("response body parse: {e}")))?;

        let token_count = resp_json
            .get("usage")
            .and_then(|u| u.get("total_tokens"))
            .and_then(|t| t.as_u64())
            .map(|n| n as u32)
            .unwrap_or(0);

        let plan = self.parse_decompose_response(&resp_json, candidate_skills)?;

        // 校验 1:max_total_steps ≤ 20
        if plan.max_total_steps > MAX_TOTAL_STEPS_HARD_LIMIT {
            return Err(LlmError::Parse(format!(
                "max_total_steps {} exceeds hard limit {}",
                plan.max_total_steps, MAX_TOTAL_STEPS_HARD_LIMIT
            )));
        }

        // 校验 2:所有 skill_id 在 candidate_skills 中
        let candidate_ids: std::collections::HashSet<&str> =
            candidate_skills.iter().map(|s| s.id.as_str()).collect();
        for node in &plan.nodes {
            if !candidate_ids.contains(node.skill_id.as_str()) {
                return Err(LlmError::Parse(format!(
                    "LLM returned illegal skill_id '{}' not in candidate_skills",
                    node.skill_id
                )));
            }
        }

        // 校验 3:SlotTemplateEngine::validate_dag
        SlotTemplateEngine::validate_dag(&plan).map_err(|e| {
            LlmError::Parse(format!("template validation failed: {}", e))
        })?;

        // 校验 4:DagPlan 内置校验
        plan.validate_edges().map_err(|e| {
            LlmError::Parse(format!("edge validation failed: {}", e))
        })?;
        plan.validate_loop_specs().map_err(|e| {
            LlmError::Parse(format!("loop spec validation failed: {}", e))
        })?;

        let stats = crate::llm::types::DecomposeStats {
            llm_model: self.model.clone(),
            latency_ms,
            token_count,
        };
        Ok((plan, stats))
    }

    /// 构建 DAG 拆解的 system prompt(中文约束)。
    #[cfg(feature = "llm")]
    fn build_decompose_system_prompt(
        &self,
        skills: &[SkillManifest],
        user_slots: &[ExtractedSlot],
    ) -> String {
        let mut s = String::from(
            "你是 VoicePilot 的 DAG 拆解器,从用户语音转写文本中识别要执行的多步 Skill 编排。\n\n\
             候选 Skill 列表:\n",
        );
        for skill in skills {
            let input_keys: Vec<&String> = skill.inputs.keys().collect();
            s.push_str(&format!(
                "- id: {}\n  title: {}\n  description: {}\n  intent_examples: {:?}\n  inputs: {:?}\n\n",
                skill.id, skill.title, skill.description, skill.intent_examples, input_keys
            ));
        }
        s.push_str(&format!("\n用户已填 Slot 列表:{:?}\n\n", user_slots));
        s.push_str(
            "约束:\n\
             1. 不得引用未在上述 candidate_skills 中的 skill_id\n\
             2. input_template.template 中所有 ${...} 必须指向合法 scope:\n\
                - ${prev.output.xxx} — 紧邻上游节点(拓扑序前驱)的 output.xxx\n\
                - ${n1.output.xxx} — 指定节点 n1 的 output.xxx\n\
                - ${user.xxx} — 用户审批阶段填的 Slot\n\
                - ${item} / ${item.xxx} — 循环变量(仅在循环节点内合法)\n\
             3. 循环节点必须填 max_iterations,默认 10,硬上限 50\n\
             4. max_total_steps 硬上限 20(防爆炸 DAG)\n\
             5. 若用户意图只需单 Skill,返回单节点 DAG(不强制多步)\n\
             6. edges 中 from / to 必须在 nodes 中存在\n\
             7. 不得编造未在 candidate_skills 中的工具或动作",
        );
        s
    }

    /// 构建 decompose_to_dag function calling schema(spec §2.2)。
    #[cfg(feature = "llm")]
    fn build_decompose_tool_schema(&self) -> serde_json::Value {
        json!([{
            "type": "function",
            "function": {
                "name": "decompose_to_dag",
                "description": "Decompose user voice transcription into a multi-step DAG plan",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "nodes": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "node_id": {"type": "string"},
                                    "skill_id": {"type": "string"},
                                    "input_template": {
                                        "type": "object",
                                        "properties": {
                                            "kind": {"type": "string"},
                                            "template": {"type": "string"}
                                        },
                                        "required": ["kind", "template"]
                                    },
                                    "risk_ceiling": {"type": "string", "enum": ["E0","E1","E2","E3"]}
                                },
                                "required": ["node_id", "skill_id", "input_template", "risk_ceiling"]
                            }
                        },
                        "edges": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "from": {"type": "string"},
                                    "to": {"type": "string"},
                                    "port_binding": {"type": ["string", "null"]}
                                },
                                "required": ["from", "to"]
                            }
                        },
                        "loop_specs": {
                            "type": "object"
                        },
                        "max_total_steps": {"type": "integer", "maximum": 20}
                    },
                    "required": ["nodes", "edges", "max_total_steps"]
                }
            }
        }])
    }

    /// 解析 LLM `/chat/completions` 响应为 DagPlan(spec §2.2)。
    ///
    /// 期望响应结构(OpenAI 兼容):
    /// ```json
    /// {
    ///   "choices": [{
    ///     "message": {
    ///       "tool_calls": [{
    ///         "function": {
    ///           "name": "decompose_to_dag",
    ///           "arguments": "{\"nodes\":[...],\"edges\":[...],...}"
    ///         }
    ///       }]
    ///     }
    ///   }]
    /// }
    /// ```
    ///
    /// `arguments` 是 JSON 字符串(OpenAI 规范),需二次 `serde_json::from_str`。
    /// 本方法只解析结构,不做语义校验(校验在 `decompose_to_dag_traced` 主体中)。
    #[cfg(feature = "llm")]
    fn parse_decompose_response(
        &self,
        resp: &serde_json::Value,
        _candidate_skills: &[SkillManifest],
    ) -> LlmResult<crate::skills::dag_types::DagPlan> {
        use crate::skills::dag_types::DagPlan;

        let arguments_str = resp
            .pointer("/choices/0/message/tool_calls/0/function/arguments")
            .and_then(|a| a.as_str())
            .ok_or_else(|| {
                LlmError::Parse(
                    "response missing choices[0].message.tool_calls[0].function.arguments".into(),
                )
            })?;

        let plan: DagPlan = serde_json::from_str(arguments_str).map_err(|e| {
            LlmError::Parse(format!("failed to parse arguments as DagPlan: {}", e))
        })?;

        Ok(plan)
    }

    /// W8 Plan 3 Task 7:失败归因 LLM 调用(spec §2.5)。
    ///
    /// 把 step 的 audit_logs 喂给 LLM,让其归因失败原因。
    /// System prompt 约束:仅基于事实,不得编造;无法归因 → category=unknown + confidence<0.5。
    ///
    /// 参数:
    /// - `step`:失败的 StepRecord(含 step_id / status / task_id)
    /// - `audit_logs`:step 相关审计事件列表(由调用方从 kernel.list_audit_for_task + filter 获取)
    ///
    /// 返回 `LlmResult<LlmAnalysis>`:
    /// - Ok(LlmAnalysis) — LLM 成功归因
    /// - Err(LlmError::Http) — HTTP 失败(调用方回退 structured_only)
    /// - Err(LlmError::Parse) — JSON 解析失败 / 非法 category
    /// - Err(LlmError::NotConfigured) — LLM 未配置
    #[cfg(feature = "llm")]
    pub async fn explain_failure(
        &self,
        step: &crate::repo::step_repo::StepRecord,
        audit_logs: &[crate::audit::AuditEvent],
    ) -> LlmResult<crate::skills::task_explain::LlmAnalysis> {
        if !self.is_enabled() {
            return Err(LlmError::NotConfigured);
        }

        let system_prompt = self.build_explain_system_prompt();
        let user_content = self.build_explain_user_content(step, audit_logs);
        let tools = self.build_explain_tool_schema();
        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_content},
            ],
            "tools": tools,
            "tool_choice": {"type": "function", "function": {"name": "explain_failure"}},
            "temperature": 0.1,
        });

        let url = format!("{}/chat/completions", self.base_url);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    LlmError::Timeout(self.timeout)
                } else {
                    LlmError::Http(e.to_string())
                }
            })?;

        if !resp.status().is_success() {
            return Err(LlmError::Http(format!("HTTP {}", resp.status())));
        }

        let resp_json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| LlmError::Parse(format!("response body parse: {e}")))?;

        self.parse_explain_response(&resp_json)
    }

    /// 构建失败归因的 system prompt(中文,spec §2.5 约束)。
    #[cfg(feature = "llm")]
    fn build_explain_system_prompt(&self) -> String {
        String::from(
            "你是 VoicePilot 的失败归因器,基于审计日志分析步骤失败原因。\n\n\
             约束:\n\
             1. 仅基于 audit_logs 中的事实,不得编造未记录的事件\n\
             2. 若无法归因,返回 category=\"unknown\" + confidence<0.5,不强行解释\n\
             3. root_cause_zh 用中文,简洁(≤ 200 字)\n\
             4. category 必须是以下之一:mcp_unavailable / path_not_allowed / approval_denied / network_error / unknown\n\
             5. suggested_fix 可选,若有明确建议则填写\n\n\
             category 含义:\n\
             - mcp_unavailable:MCP 服务器未启动 / 命令不存在 / spawn 失败\n\
             - path_not_allowed:文件路径不在 allowed_paths 白名单\n\
             - approval_denied:用户拒绝审批\n\
             - network_error:网络请求失败 / 超时\n\
             - unknown:无法归因"
        )
    }

    /// 构建用户消息:序列化 step + audit_logs 为 JSON 字符串。
    ///
    /// 注:`StepRecord` 没有 `error_message` 字段(W7 既有结构),
    /// 此处用 `serde_json::Value::Null` 占位以保持 JSON 形状稳定,
    /// LLM 可从 audit_logs 中提取错误细节。
    #[cfg(feature = "llm")]
    fn build_explain_user_content(
        &self,
        step: &crate::repo::step_repo::StepRecord,
        audit_logs: &[crate::audit::AuditEvent],
    ) -> String {
        let step_json = serde_json::json!({
            "step_id": step.step_id,
            "task_id": step.task_id,
            "status": step.status.as_str(),
            "evidence_strength": step.evidence_strength,
            "error_message": serde_json::Value::Null,
        });
        let logs_json: Vec<serde_json::Value> = audit_logs.iter().map(|e| {
            serde_json::json!({
                "event_type": e.event_type,
                "details": e.details,
                "timestamp": e.timestamp.to_rfc3339(),
            })
        }).collect();
        format!(
            "失败的步骤:\n{}\n\n审计日志:\n{}",
            serde_json::to_string_pretty(&step_json).unwrap_or_default(),
            serde_json::to_string_pretty(&logs_json).unwrap_or_default()
        )
    }

    /// 构建 explain_failure function calling schema。
    #[cfg(feature = "llm")]
    fn build_explain_tool_schema(&self) -> serde_json::Value {
        json!([{
            "type": "function",
            "function": {
                "name": "explain_failure",
                "description": "Analyze step failure from audit logs and return root cause in Chinese",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "root_cause_zh": {
                            "type": "string",
                            "description": "中文归因,≤ 200 字,仅基于 audit_logs 事实"
                        },
                        "category": {
                            "type": "string",
                            "enum": ["mcp_unavailable", "path_not_allowed", "approval_denied", "network_error", "unknown"]
                        },
                        "suggested_fix": {
                            "type": ["string", "null"],
                            "description": "可选的修复建议"
                        },
                        "confidence": {
                            "type": "number",
                            "minimum": 0,
                            "maximum": 1
                        }
                    },
                    "required": ["root_cause_zh", "category", "confidence"]
                }
            }
        }])
    }

    /// 解析 LLM 响应 → LlmAnalysis。
    /// 非法 category → Err(LlmError::Parse)(调用方回退 structured_only)。
    #[cfg(feature = "llm")]
    fn parse_explain_response(
        &self,
        resp: &serde_json::Value,
    ) -> LlmResult<crate::skills::task_explain::LlmAnalysis> {
        use crate::skills::explanation_repo::FailureCategory;

        let tool_call = resp
            .pointer("/choices/0/message/tool_calls/0")
            .ok_or_else(|| LlmError::Parse("missing tool_calls[0]".to_string()))?;
        let args_str = tool_call
            .pointer("/function/arguments")
            .and_then(|v| v.as_str())
            .ok_or_else(|| LlmError::Parse("missing function.arguments".to_string()))?;
        let args: serde_json::Value = serde_json::from_str(args_str)
            .map_err(|e| LlmError::Parse(format!("arguments parse: {e}")))?;

        let root_cause_zh = args
            .get("root_cause_zh")
            .and_then(|v| v.as_str())
            .ok_or_else(|| LlmError::Parse("missing root_cause_zh".to_string()))?
            .to_string();

        let category_str = args
            .get("category")
            .and_then(|v| v.as_str())
            .ok_or_else(|| LlmError::Parse("missing category".to_string()))?;
        let category = FailureCategory::parse_str(category_str)
            .ok_or_else(|| LlmError::Parse(format!("invalid category: {}", category_str)))?;

        let suggested_fix = args
            .get("suggested_fix")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let confidence = args
            .get("confidence")
            .and_then(|v| v.as_f64())
            .map(|f| f as f32)
            .unwrap_or(0.0);

        Ok(crate::skills::task_explain::LlmAnalysis {
            root_cause_zh,
            category,
            suggested_fix,
            confidence,
        })
    }
}

/// W8 Plan 2 Task 8:记录 `llm_decompose_called` 审计事件(硬约束)。
///
/// 必须携带字段(project_memory):
/// - `plan_id` — LLM 返回的 DagPlan.plan_id(成功路径)或 "unknown"(失败路径)
/// - `llm_model` — LLM 模型名(来自 DecomposeStats)
/// - `latency_ms` — LLM 调用耗时毫秒
/// - `token_count` — LLM 响应 token 数
///
/// 调用方(router_bridge Plan 4)在 LLM 调用后(无论成功 / 失败)调本方法。
/// 失败路径下 `stats` 可填零值(`latency_ms` 测到错误为止,`token_count = 0`)。
///
/// 注:`task_id` 是 root task 的 ID(FK 约束 audit_logs.task_id REFERENCES tasks)。
/// 调用方必须先 `kernel.create_task(task_id, ...)` 再调本方法。
#[cfg(feature = "llm")]
pub fn record_llm_decompose_called(
    kernel: &crate::kernel::TrustKernel,
    task_id: &str,
    plan_id: &str,
    stats: &crate::llm::types::DecomposeStats,
) -> crate::error::Result<()> {
    kernel.audit_append_external(
        task_id,
        None,
        "llm_decompose_called",
        serde_json::json!({
            "plan_id": plan_id,
            "llm_model": stats.llm_model,
            "latency_ms": stats.latency_ms,
            "token_count": stats.token_count,
        }),
    )
}

/// W9 Plan 3: 为 DagPlan 中所有 `TemplateExpr::Literal(s)` 值标记 `llm_output` taint。
///
/// spec §2.3 传播规则:LLM 拆解产生的 literal 值视为 `llm_output` provenance,
/// 防止 LLM 注入的字符串未经审批流入文件系统写入(`LocalFile` sink)。
///
/// 实现要点:
///   - 递归遍历 `SlotTemplate.template`(AST),对每个 `Literal(s)` 计算 SHA256
///   - `source_ref = "{plan_id}:{node_id}"`(便于按 plan + node 回溯)
///   - `provenance = "llm_output"` / `taints = ["llm_output"]`
///   - upsert 通过 `TaintRepo`(自动合并同 hash 的 taints)
///   - 审计:`taint_propagated` 事件(details 仅含 hash + 标签,不含原始 value,spec §6.2)
///
/// 调用方(`router_bridge.rs`)在 `decompose_to_dag_traced` 成功后调本方法。
/// 设计权衡:不修改 `decompose_to_dag` / `decompose_to_dag_traced` 签名(避免
/// 破坏 12+ 测试 callsite),与既有 `record_llm_decompose_called` 同模式——
/// 独立函数接 `&TrustKernel`,由 router_bridge 在 LLM 调用后主动调。
#[cfg(feature = "llm")]
pub fn tag_dag_plan_literals(
    kernel: &crate::kernel::TrustKernel,
    task_id: &str,
    plan: &crate::skills::dag_types::DagPlan,
) -> crate::error::Result<()> {
    use crate::policy::taint_repo::{compute_value_hash, make_taint_record, TaintRepo};
    use crate::skills::template::TemplateExpr;

    /// 递归遍历 TemplateExpr,对每个 Literal(s) 调用 f(&s)。
    fn collect_literals<F: FnMut(&str)>(expr: &TemplateExpr, f: &mut F) {
        match expr {
            TemplateExpr::Literal(s) => f(s),
            TemplateExpr::Var(_) => {}
            TemplateExpr::Concat(parts) => {
                for p in parts {
                    collect_literals(p, f);
                }
            }
            TemplateExpr::Filter { source, .. } => {
                collect_literals(source, f);
            }
        }
    }

    let repo = TaintRepo::new();
    for node in &plan.nodes {
        let mut hashes: Vec<(String, String)> = Vec::new(); // (literal_str, hash)
        collect_literals(&node.input_template.template, &mut |s| {
            let value_json = serde_json::Value::String(s.to_string());
            let hash = compute_value_hash(&value_json);
            hashes.push((s.to_string(), hash));
        });

        for (literal_str, hash) in hashes {
            let record = make_taint_record(
                hash.clone(),
                "llm_output".to_string(),
                vec!["llm_output".to_string()],
                Some(format!("{}:{}", plan.plan_id, node.node_id)),
            );
            {
                let conn = kernel.conn();
                repo.upsert(&conn, &record)?;
            }
            kernel.audit_append_external(
                task_id,
                None,
                "taint_propagated",
                serde_json::json!({
                    "source_ref": format!("{}:{}", plan.plan_id, node.node_id),
                    "input_hash": null,
                    "output_hash": hash,
                    "taints": ["llm_output"],
                    "literal_len": literal_str.len(),
                }),
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::manifest::files_organize_manifest;

    #[test]
    fn disabled_client_is_not_enabled() {
        let c = LlmClient::disabled();
        assert!(!c.is_enabled());
    }

    #[test]
    fn new_client_with_api_key_is_enabled() {
        let c = LlmClient::new("https://api.deepseek.com/v1", "sk-test", "deepseek-chat");
        assert!(c.is_enabled());
    }

    #[test]
    fn new_client_without_api_key_is_not_enabled() {
        let c = LlmClient::new("https://api.deepseek.com/v1", "", "deepseek-chat");
        assert!(!c.is_enabled());
    }

    #[tokio::test]
    async fn classify_and_extract_returns_not_configured_when_disabled() {
        let client = LlmClient::disabled();
        let skills = vec![files_organize_manifest()];
        let result = client.classify_and_extract("整理下载目录", &skills).await;
        assert!(matches!(result, Err(LlmError::NotConfigured)));
    }

    #[tokio::test]
    async fn classify_and_extract_parses_valid_response() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let body = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "route_skill",
                            "arguments": "{\"matched_skill_id\":\"files.organize\",\"confidence\":0.9,\"slots\":[{\"kind\":\"path\",\"raw\":\"C:\\\\Downloads\",\"high_risk\":true}],\"reasoning\":\"user wants to organize files\"}"
                        }
                    }]
                }
            }]
        });
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("authorization", "Bearer sk-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let client = LlmClient::new(&server.uri(), "sk-test", "deepseek-chat");
        let skills = vec![files_organize_manifest()];
        let resp = client
            .classify_and_extract("整理下载目录", &skills)
            .await
            .unwrap();
        assert_eq!(resp.matched_skill_id.as_deref(), Some("files.organize"));
        assert!((resp.confidence - 0.9).abs() < 0.01);
        assert_eq!(resp.slots.len(), 1);
        assert_eq!(resp.slots[0].kind, "path");
        assert!(resp.slots[0].high_risk);
    }

    #[tokio::test]
    async fn classify_and_extract_returns_error_on_401() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let client = LlmClient::new(&server.uri(), "sk-invalid", "deepseek-chat");
        let skills = vec![];
        let result = client.classify_and_extract("test", &skills).await;
        assert!(matches!(result, Err(LlmError::Http(_))));
    }

    #[tokio::test]
    async fn classify_and_extract_returns_parse_error_when_tool_calls_missing() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let body = json!({
            "choices": [{
                "message": {"role": "assistant", "content": "no tool call"}
            }]
        });
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let client = LlmClient::new(&server.uri(), "sk-test", "deepseek-chat");
        let skills = vec![];
        let result = client.classify_and_extract("test", &skills).await;
        assert!(matches!(result, Err(LlmError::Parse(_))));
    }
}
