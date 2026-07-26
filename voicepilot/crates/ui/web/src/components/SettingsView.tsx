import { useEffect, useState } from "react";
import { getSettings, updateSettings } from "../api";
import type { Settings } from "../types";

export function SettingsView(): JSX.Element {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    getSettings()
      .then(setSettings)
      .catch((e) => setError(String(e)));
  }, []);

  const handleField = <K extends keyof Settings>(key: K, value: Settings[K]): void => {
    if (settings) {
      setSettings({ ...settings, [key]: value });
      setSaved(false);
    }
  };

  const handleSave = (): void => {
    if (!settings) return;
    setSaving(true);
    setError(null);
    updateSettings(settings)
      .then(() => {
        setSaved(true);
        setSaving(false);
      })
      .catch((e) => {
        setError(String(e));
        setSaving(false);
      });
  };

  if (settings === null) {
    return <div className="view-container" role="status" aria-live="polite">加载设置中…</div>;
  }

  return (
    <section className="view-container settings-form" aria-labelledby="settings-heading">
      <h2 id="settings-heading">§ 8.3 Settings</h2>

      <fieldset className="settings-fieldset">
        <legend>语音配置</legend>
        <div className="form-row">
          <label htmlFor="voice_model_path">模型路径</label>
          <input
            id="voice_model_path"
            type="text"
            value={settings.voice_model_path}
            onChange={(e) => handleField("voice_model_path", e.target.value)}
          />
        </div>
        <div className="form-row">
          <label htmlFor="voice_language">语言(空=自动)</label>
          <input
            id="voice_language"
            type="text"
            value={settings.voice_language ?? ""}
            onChange={(e) => handleField("voice_language", e.target.value || null)}
          />
        </div>
        <div className="form-row">
          <label htmlFor="voice_threads">线程数</label>
          <input
            id="voice_threads"
            type="number"
            min={1}
            max={16}
            value={settings.voice_threads}
            onChange={(e) => handleField("voice_threads", Number(e.target.value))}
          />
        </div>
      </fieldset>

      <fieldset className="settings-fieldset">
        <legend>VAD 配置</legend>
        <div className="form-row">
          <label htmlFor="vad_energy_threshold">能量阈值</label>
          <input
            id="vad_energy_threshold"
            type="number"
            step="10"
            value={settings.vad_energy_threshold}
            onChange={(e) => handleField("vad_energy_threshold", Number(e.target.value))}
          />
        </div>
        <div className="form-row">
          <label htmlFor="vad_max_silence_ms">静音超时(ms)</label>
          <input
            id="vad_max_silence_ms"
            type="number"
            value={settings.vad_max_silence_ms}
            onChange={(e) => handleField("vad_max_silence_ms", Number(e.target.value))}
          />
        </div>
        <div className="form-row">
          <label htmlFor="vad_min_speech_ms">最短语音(ms)</label>
          <input
            id="vad_min_speech_ms"
            type="number"
            value={settings.vad_min_speech_ms}
            onChange={(e) => handleField("vad_min_speech_ms", Number(e.target.value))}
          />
        </div>
        <div className="form-row">
          <label htmlFor="voice_max_duration_ms">最长录音(ms)</label>
          <input
            id="voice_max_duration_ms"
            type="number"
            value={settings.voice_max_duration_ms}
            onChange={(e) => handleField("voice_max_duration_ms", Number(e.target.value))}
          />
        </div>
        <div className="form-row">
          <label htmlFor="voice_chunk_duration_ms">块大小(ms)</label>
          <input
            id="voice_chunk_duration_ms"
            type="number"
            value={settings.voice_chunk_duration_ms}
            onChange={(e) => handleField("voice_chunk_duration_ms", Number(e.target.value))}
          />
        </div>
      </fieldset>

      <fieldset className="settings-fieldset">
        <legend>隐私与补偿</legend>
        <div className="form-row checkbox-row">
          <input
            id="privacy_mode"
            type="checkbox"
            checked={settings.privacy_mode}
            onChange={(e) => handleField("privacy_mode", e.target.checked)}
          />
          <label htmlFor="privacy_mode">隐私模式(禁用审计详情记录)</label>
        </div>
        <div className="form-row">
          <label htmlFor="compensation_ttl_hours">Compensation TTL(小时)</label>
          <input
            id="compensation_ttl_hours"
            type="number"
            min={1}
            value={settings.compensation_ttl_hours}
            onChange={(e) => handleField("compensation_ttl_hours", Number(e.target.value))}
          />
        </div>
      </fieldset>

      {/* W6c P1 #1:TTS 配置(VP-FR-002 语音反馈)*/}
      <fieldset className="settings-fieldset">
        <legend>TTS 配置</legend>
        <div className="form-row checkbox-row">
          <input
            id="tts_enabled"
            type="checkbox"
            checked={settings.tts_enabled}
            onChange={(e) => handleField("tts_enabled", e.target.checked)}
          />
          <label htmlFor="tts_enabled">启用语音反馈</label>
        </div>
        <div className="form-row">
          <label htmlFor="tts_model_path">TTS 模型路径(空=使用默认)</label>
          <input
            id="tts_model_path"
            type="text"
            value={settings.tts_model_path}
            onChange={(e) => handleField("tts_model_path", e.target.value)}
          />
        </div>
      </fieldset>

      {/* W7:LLM 配置(OpenAI 兼容,默认 DeepSeek)。
          privacy_mode=true 时整段禁用(后端 rebuild_llm_client 强制 disabled)。*/}
      <fieldset className="settings-fieldset">
        <legend>LLM 配置</legend>
        <div className="form-row checkbox-row">
          <input
            id="llm_enabled"
            type="checkbox"
            checked={settings.llm_enabled}
            onChange={(e) => handleField("llm_enabled", e.target.checked)}
            disabled={settings.privacy_mode}
            aria-describedby="llm-enabled-hint"
          />
          <label htmlFor="llm_enabled">启用云端 LLM(用于意图分类与 Slot 提取)</label>
        </div>
        <p id="llm-enabled-hint" className="settings-hint">
          LLM 仅在关键词路由未命中时调用,Skill 执行不调 LLM。
        </p>
        {settings.privacy_mode && (
          <p className="settings-hint settings-warn" role="alert">
            隐私模式已启用,LLM 不可用
          </p>
        )}
        <div className="form-row">
          <label htmlFor="llm_api_key">API Key</label>
          <input
            id="llm_api_key"
            type="password"
            value={settings.llm_api_key}
            onChange={(e) => handleField("llm_api_key", e.target.value)}
            placeholder="sk-..."
            autoComplete="off"
            disabled={settings.privacy_mode || !settings.llm_enabled}
          />
        </div>
        <div className="form-row">
          <label htmlFor="llm_base_url">Base URL</label>
          <input
            id="llm_base_url"
            type="text"
            value={settings.llm_base_url}
            onChange={(e) => handleField("llm_base_url", e.target.value)}
            placeholder="https://api.deepseek.com/v1"
            disabled={settings.privacy_mode || !settings.llm_enabled}
          />
        </div>
        <div className="form-row">
          <label htmlFor="llm_model">模型名</label>
          <input
            id="llm_model"
            type="text"
            value={settings.llm_model}
            onChange={(e) => handleField("llm_model", e.target.value)}
            placeholder="deepseek-chat"
            disabled={settings.privacy_mode || !settings.llm_enabled}
          />
        </div>
        <div className="form-row">
          <a
            href={settings.llm_provider_url}
            target="_blank"
            rel="noopener noreferrer"
            className="settings-link"
          >
            获取 API Key
          </a>
        </div>
        <details className="settings-details">
          <summary>常见 provider 配置</summary>
          <ul>
            <li>DeepSeek: base_url=<code>https://api.deepseek.com/v1</code>, model=<code>deepseek-chat</code></li>
            <li>OpenAI: base_url=<code>https://api.openai.com/v1</code>, model=<code>gpt-4o-mini</code></li>
            <li>通义千问: base_url=<code>https://dashscope.aliyuncs.com/compatible-mode/v1</code>, model=<code>qwen-turbo</code></li>
            <li>Kimi: base_url=<code>https://api.moonshot.cn/v1</code>, model=<code>moonshot-v1-8k</code></li>
          </ul>
        </details>
      </fieldset>

      {/* W7 Plan 4:UIA 白名单(quick.app_control / note.capture 可启动的应用列表)。
          逗号分隔输入,后端 JSON 数组持久化。uia feature 关闭时仍可编辑(数据无害)。*/}
      <fieldset className="settings-fieldset">
        <legend>UIA 应用白名单</legend>
        <div className="form-row">
          <label htmlFor="uia_allowed_apps">允许的应用(逗号分隔)</label>
          <input
            id="uia_allowed_apps"
            type="text"
            value={settings.uia_allowed_apps.join(", ")}
            onChange={(e) =>
              handleField(
                "uia_allowed_apps",
                e.target.value
                  .split(",")
                  .map((s) => s.trim())
                  .filter((s) => s.length > 0)
              )
            }
            placeholder="notepad, explorer, calc"
            aria-describedby="uia-allowed-apps-hint"
          />
        </div>
        <p id="uia-allowed-apps-hint" className="settings-hint">
          quick.app_control 与 note.capture 仅能启动此列表内的应用。超出列表的 launch 需 PerStep 审批。
        </p>
      </fieldset>

      <div className="form-actions">
        <button type="button" onClick={handleSave} disabled={saving}>
          {saving ? "保存中…" : "保存设置"}
        </button>
        {saved && <span className="save-success" role="status">✓ 已保存</span>}
        {error && <span className="form-error" role="alert">错误:{error}</span>}
      </div>
    </section>
  );
}
