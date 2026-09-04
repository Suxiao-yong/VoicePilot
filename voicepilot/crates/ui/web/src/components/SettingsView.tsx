import { useEffect, useState } from "react";
import { getSettings, updateSettings, testLlm } from "../api";
import type { Settings, SettingsUpdate, LlmTestResult } from "../types";

export function SettingsView(): JSX.Element {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  // Wave 3:API key 不预填、不回读。用户只在新 key 输入框填写新值;
  // 空 = 保持现有 key。清除用独立开关。
  const [newApiKey, setNewApiKey] = useState("");
  const [clearApiKey, setClearApiKey] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<LlmTestResult | null>(null);

  useEffect(() => {
    getSettings()
      .then(setSettings)
      .catch((e) => setError(String(e)));
  }, []);

  const handleField = <K extends keyof Settings>(
    key: K,
    value: Settings[K],
  ): void => {
    if (settings) {
      setSettings({ ...settings, [key]: value });
      setSaved(false);
    }
  };

  // 3-5:数值字段 clamp(后端只 parse 不校验范围,空输入 Number("")=0 会存成非法值)
  const NUMERIC_RANGES: {
    key: keyof Settings;
    min: number;
    max: number;
  }[] = [
    { key: "voice_threads", min: 1, max: 16 },
    { key: "vad_energy_threshold", min: 0, max: 100000 },
    { key: "vad_max_silence_ms", min: 100, max: 30000 },
    { key: "vad_min_speech_ms", min: 50, max: 5000 },
    { key: "voice_max_duration_ms", min: 1000, max: 600000 },
    { key: "voice_chunk_duration_ms", min: 10, max: 60000 },
    { key: "compensation_ttl_hours", min: 1, max: 8760 },
  ];

  const handleNumericField = (key: keyof Settings, raw: string): void => {
    const range = NUMERIC_RANGES.find((r) => r.key === key);
    const parsed = Number(raw);
    if (Number.isNaN(parsed)) return;
    handleField(
      key,
      Math.min(
        range ? range.max : Number.MAX_SAFE_INTEGER,
        Math.max(range ? range.min : 0, parsed),
      ) as Settings[typeof key],
    );
  };

  const handleTest = (): void => {
    if (!settings || testing) return;
    setTesting(true);
    setTestResult(null);
    setError(null);
    // 用表单当前值测：新 key 优先，否则测已存 key；保存前可测。
    testLlm({
      base_url: settings.llm_base_url,
      model: settings.llm_model,
      api_key: newApiKey.trim() !== "" ? newApiKey.trim() : null,
    })
      .then((r) => {
        setTestResult(r);
        setTesting(false);
      })
      .catch((e) => {
        setError(String(e));
        setTesting(false);
      });
  };

  const handleSave = (): void => {
    if (!settings) return;
    setSaving(true);
    setError(null);
    // 组装写入 DTO:非 secret 字段来自读取视图;secret 字段显式。
    const update: SettingsUpdate = {
      voice_model_path: settings.voice_model_path,
      voice_language: settings.voice_language,
      voice_threads: settings.voice_threads,
      vad_energy_threshold: settings.vad_energy_threshold,
      vad_max_silence_ms: settings.vad_max_silence_ms,
      vad_min_speech_ms: settings.vad_min_speech_ms,
      voice_max_duration_ms: settings.voice_max_duration_ms,
      voice_chunk_duration_ms: settings.voice_chunk_duration_ms,
      privacy_mode: settings.privacy_mode,
      compensation_ttl_hours: settings.compensation_ttl_hours,
      tts_enabled: settings.tts_enabled,
      tts_model_path: settings.tts_model_path,
      llm_enabled: settings.llm_enabled,
      llm_base_url: settings.llm_base_url,
      llm_model: settings.llm_model,
      llm_provider_url: settings.llm_provider_url,
      llm_api_key: newApiKey.trim() !== "" ? newApiKey.trim() : null,
      clear_llm_api_key: clearApiKey,
      uia_allowed_apps: settings.uia_allowed_apps,
    };
    updateSettings(update)
      .then(() => {
        setSaved(true);
        setSaving(false);
        // 刷新读取视图(同步 llm_api_key_present)并清空 secret 输入。
        setNewApiKey("");
        setClearApiKey(false);
        return getSettings()
          .then(setSettings)
          .catch(() => undefined);
      })
      .catch((e) => {
        setError(String(e));
        setSaving(false);
      });
  };

  if (settings === null) {
    return (
      <div role="status" aria-live="polite">
        加载设置中…
      </div>
    );
  }

  return (
    <section aria-labelledby="settings-heading">
      <header className="view-head">
        <h1 id="settings-heading" className="view-title">
          <span className="view-kicker">Settings</span>
          设置
        </h1>
        <p className="view-desc">
          语音、隐私、LLM 与权限配置。修改后点击保存生效。
        </p>
      </header>

      <div className="settings-grid">
        <fieldset className="settings-group">
          <legend>语音识别</legend>
          <div className="field">
            <label className="field-label" htmlFor="voice_model_path">
              模型路径
            </label>
            <input
              id="voice_model_path"
              className="field-input mono"
              type="text"
              value={settings.voice_model_path}
              onChange={(e) => handleField("voice_model_path", e.target.value)}
            />
          </div>
          <div className="field">
            <label className="field-label" htmlFor="voice_language">
              语言（空 = 自动）
            </label>
            <input
              id="voice_language"
              className="field-input mono"
              type="text"
              value={settings.voice_language ?? ""}
              onChange={(e) =>
                handleField("voice_language", e.target.value || null)
              }
            />
          </div>
          <div className="field">
            <label className="field-label" htmlFor="voice_threads">
              线程数
            </label>
            <input
              id="voice_threads"
              className="field-input mono"
              type="number"
              min={1}
              max={16}
              value={settings.voice_threads}
              onChange={(e) =>
                handleNumericField("voice_threads", e.target.value)
              }
            />
          </div>
        </fieldset>

        <fieldset className="settings-group">
          <legend>VAD 检测</legend>
          <div className="field">
            <label className="field-label" htmlFor="vad_energy_threshold">
              能量阈值
            </label>
            <input
              id="vad_energy_threshold"
              className="field-input mono"
              type="number"
              step="10"
              value={settings.vad_energy_threshold}
              onChange={(e) =>
                handleNumericField("vad_energy_threshold", e.target.value)
              }
            />
          </div>
          <div className="field">
            <label className="field-label" htmlFor="vad_max_silence_ms">
              静音超时（ms）
            </label>
            <input
              id="vad_max_silence_ms"
              className="field-input mono"
              type="number"
              value={settings.vad_max_silence_ms}
              onChange={(e) =>
                handleNumericField("vad_max_silence_ms", e.target.value)
              }
            />
          </div>
          <div className="field">
            <label className="field-label" htmlFor="vad_min_speech_ms">
              最短语音（ms）
            </label>
            <input
              id="vad_min_speech_ms"
              className="field-input mono"
              type="number"
              value={settings.vad_min_speech_ms}
              onChange={(e) =>
                handleNumericField("vad_min_speech_ms", e.target.value)
              }
            />
          </div>
          <div className="field">
            <label className="field-label" htmlFor="voice_max_duration_ms">
              最长录音（ms）
            </label>
            <input
              id="voice_max_duration_ms"
              className="field-input mono"
              type="number"
              value={settings.voice_max_duration_ms}
              onChange={(e) =>
                handleNumericField("voice_max_duration_ms", e.target.value)
              }
            />
          </div>
          <div className="field">
            <label className="field-label" htmlFor="voice_chunk_duration_ms">
              块大小（ms）
            </label>
            <input
              id="voice_chunk_duration_ms"
              className="field-input mono"
              type="number"
              value={settings.voice_chunk_duration_ms}
              onChange={(e) =>
                handleNumericField("voice_chunk_duration_ms", e.target.value)
              }
            />
          </div>
        </fieldset>

        <fieldset className="settings-group">
          <legend>隐私与补偿</legend>
          <label className="field-check" htmlFor="privacy_mode">
            <input
              id="privacy_mode"
              type="checkbox"
              checked={settings.privacy_mode}
              onChange={(e) => handleField("privacy_mode", e.target.checked)}
            />
            隐私模式（禁用审计详情记录）
          </label>
          <div className="field">
            <label className="field-label" htmlFor="compensation_ttl_hours">
              补偿保留时间（小时）
            </label>
            <input
              id="compensation_ttl_hours"
              className="field-input mono"
              type="number"
              min={1}
              value={settings.compensation_ttl_hours}
              onChange={(e) =>
                handleNumericField("compensation_ttl_hours", e.target.value)
              }
            />
          </div>
        </fieldset>

        <fieldset className="settings-group">
          <legend>语音反馈（TTS）</legend>
          <label className="field-check" htmlFor="tts_enabled">
            <input
              id="tts_enabled"
              type="checkbox"
              checked={settings.tts_enabled}
              onChange={(e) => handleField("tts_enabled", e.target.checked)}
            />
            启用语音反馈
          </label>
          <div className="field">
            <label className="field-label" htmlFor="tts_model_path">
              TTS 模型路径（空 = 默认）
            </label>
            <input
              id="tts_model_path"
              className="field-input mono"
              type="text"
              value={settings.tts_model_path}
              onChange={(e) => handleField("tts_model_path", e.target.value)}
            />
          </div>
        </fieldset>

        <fieldset className="settings-group">
          <legend>云端 LLM</legend>
          <label className="field-check" htmlFor="llm_enabled">
            <input
              id="llm_enabled"
              type="checkbox"
              checked={settings.llm_enabled}
              onChange={(e) => handleField("llm_enabled", e.target.checked)}
              disabled={settings.privacy_mode}
              aria-describedby="llm-enabled-hint"
            />
            启用云端 LLM（意图分类与 Slot 提取）
          </label>
          <p id="llm-enabled-hint" className="field-hint">
            LLM 仅在关键词路由未命中时调用，Skill 执行不调 LLM。
          </p>
          {settings.privacy_mode && (
            <p className="field-hint warn" role="alert">
              隐私模式已启用，LLM 不可用
            </p>
          )}
          <div className="field">
            <label className="field-label" htmlFor="llm_api_key">
              API Key（新 key，留空 = 保持不变）
            </label>
            <input
              id="llm_api_key"
              className="field-input mono"
              type="password"
              value={newApiKey}
              onChange={(e) => setNewApiKey(e.target.value)}
              placeholder={
                settings.llm_api_key_present ? "已保存，留空保持不变" : "sk-..."
              }
              autoComplete="off"
              disabled={settings.privacy_mode || !settings.llm_enabled}
            />
            <p className="field-hint" id="llm-api-key-hint">
              {settings.llm_api_key_present
                ? "已配置 API Key（存于系统凭据管理器，不回显）。"
                : "尚未配置 API Key。"}
            </p>
          </div>
          <label className="field-check" htmlFor="clear_llm_api_key">
            <input
              id="clear_llm_api_key"
              type="checkbox"
              checked={clearApiKey}
              onChange={(e) => setClearApiKey(e.target.checked)}
              disabled={
                settings.privacy_mode ||
                !settings.llm_enabled ||
                newApiKey.trim() !== ""
              }
            />
            清除已保存的 API Key
            {newApiKey.trim() !== "" && (
              <span className="field-hint" role="note">
                （已输入新 key，将覆盖清除）
              </span>
            )}
          </label>
          <div className="field">
            <label className="field-label" htmlFor="llm_base_url">
              Base URL
            </label>
            <input
              id="llm_base_url"
              className="field-input mono"
              type="text"
              value={settings.llm_base_url}
              onChange={(e) => handleField("llm_base_url", e.target.value)}
              placeholder="https://api.deepseek.com/v1"
              disabled={settings.privacy_mode || !settings.llm_enabled}
            />
          </div>
          <div className="field">
            <label className="field-label" htmlFor="llm_model">
              模型名
            </label>
            <input
              id="llm_model"
              className="field-input mono"
              type="text"
              value={settings.llm_model}
              onChange={(e) => handleField("llm_model", e.target.value)}
              placeholder="deepseek-chat"
              disabled={settings.privacy_mode || !settings.llm_enabled}
            />
          </div>
          <a
            href={settings.llm_provider_url}
            target="_blank"
            rel="noopener noreferrer"
            className="link"
          >
            获取 API Key
          </a>
          <details className="settings-details">
            <summary>常见 provider 配置</summary>
            <ul>
              <li>
                DeepSeek：base_url=<code>https://api.deepseek.com/v1</code>
                ，model=<code>deepseek-chat</code>
              </li>
              <li>
                OpenAI：base_url=<code>https://api.openai.com/v1</code>，model=
                <code>gpt-4o-mini</code>
              </li>
              <li>
                通义千问：base_url=
                <code>https://dashscope.aliyuncs.com/compatible-mode/v1</code>
                ，model=<code>qwen-turbo</code>
              </li>
              <li>
                Kimi：base_url=<code>https://api.moonshot.cn/v1</code>，model=
                <code>moonshot-v1-8k</code>
              </li>
            </ul>
          </details>
        </fieldset>

        <fieldset className="settings-group">
          <legend>UIA 应用白名单</legend>
          <div className="field">
            <label className="field-label" htmlFor="uia_allowed_apps">
              允许的应用（逗号分隔）
            </label>
            <input
              id="uia_allowed_apps"
              className="field-input mono"
              type="text"
              value={settings.uia_allowed_apps.join(", ")}
              onChange={(e) =>
                handleField(
                  "uia_allowed_apps",
                  e.target.value
                    .split(",")
                    .map((s) => s.trim())
                    .filter((s) => s.length > 0),
                )
              }
              placeholder="notepad, explorer, calc"
              aria-describedby="uia-allowed-apps-hint"
            />
          </div>
          <p id="uia-allowed-apps-hint" className="field-hint">
            quick.app_control 与 note.capture 仅能启动此列表内的应用。超出列表的
            launch 需逐步骤审批。
          </p>
        </fieldset>

        <fieldset className="settings-group">
          <legend>Playwright MCP</legend>
          <p className="field-hint">
            浏览器自动化 Skill（<code>research.save_markdown</code>、
            <code>form.prepare</code>）依赖 Playwright MCP。
          </p>
          <ul className="help-list">
            <li>
              需 Node.js ≥ 18（运行 <code>node --version</code> 验证）
            </li>
            <li>
              首次使用时 <code>npx -y @playwright/mcp@latest</code>{" "}
              会自动下载，需网络
            </li>
            <li>
              启用后浏览器实例由 MCP server 管理，关闭 VoicePilot 时自动清理
            </li>
            <li>
              故障排查：见 <code>docs/playwright-mcp-setup.md</code>
            </li>
          </ul>
        </fieldset>
      </div>

      <div className="settings-actions">
        <button
          type="button"
          className="btn btn-primary"
          onClick={handleSave}
          disabled={saving}
        >
          {saving ? "保存中…" : "保存设置"}
        </button>
        <button
          type="button"
          className="btn"
          onClick={handleTest}
          disabled={testing || saving}
          title="用当前表单值测一次云端连通性，不保存任何东西"
        >
          {testing ? "测试中…" : "测试连接"}
        </button>
        {testResult && testResult.kind === "ok" && (
          <span className="save-note" role="status">
            ✓ 连接正常（{testResult.model}，{testResult.latency_ms}ms）
          </span>
        )}
        {testResult && testResult.kind === "failed" && (
          <span className="alert alert-error" role="alert">
            连接失败：{testResult.message}
          </span>
        )}
        {saved && (
          <span className="save-note" role="status">
            ✓ 已保存
          </span>
        )}
        {error && (
          <span className="alert alert-error" role="alert">
            错误：{error}
          </span>
        )}
      </div>
    </section>
  );
}
