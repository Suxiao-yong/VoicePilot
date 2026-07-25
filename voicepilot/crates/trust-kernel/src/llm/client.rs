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
