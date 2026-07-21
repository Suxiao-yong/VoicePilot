-- W6b-2: Settings 持久化 KV 表(V1.1.2 §8.3 Settings)
-- 简单 key-value 存储,value 为 JSON 字符串。
-- key 命名空间约定:
--   voice.model_path / voice.language / voice.threads
--   voice.vad.energy_threshold / voice.vad.max_silence_ms / voice.vad.min_speech_ms
--   voice.max_duration_ms / voice.chunk_duration_ms
--   privacy.mode
--   compensation.ttl_hours
CREATE TABLE IF NOT EXISTS app_config (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
