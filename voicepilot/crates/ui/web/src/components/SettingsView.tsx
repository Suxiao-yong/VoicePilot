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
