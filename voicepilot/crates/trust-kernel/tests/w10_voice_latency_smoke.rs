//! W10 Plan 3 — P95 首字延迟基准(spec §VP-FR-001 ≤ 500ms)。
//!
//! 测量 VAD 检测首个 voiced chunk → ASR 转写完成的延迟。
//! 100 样本,P95 = 排序后索引 94,断言 ≤ 500ms。
//!
//! **#[ignore] 原因:** 需真实 sherpa-rs SenseVoice 模型(~80MB)+
//! 预录 WAV fixture。CI 无法下载,本地手动运行:
//! ```powershell
//! cargo test -p trust-kernel --features voice --test w10_voice_latency_smoke -- --ignored --nocapture
//! ```
//!
//! **运行频率(spec §5.2 v2 修订 #7):**
//! - 每次版本发布前(含 W10 Plan 6 集成验收)手动运行
//! - 在 PROGRESS.md Plan 3 段落记录最新一次运行结果(样本数、P50/P95/P99 实测值)
//! - W11+ CI 接入后,标记为 `#[ignore = "requires-sherpa-model"]`,CI workflow 单独 step 跑
//!
//! **测试策略:**
//! - 优先查找 `push_to_talk_sample.wav`(Plan 3 专用 fixture)
//! - 回退到 `w5_sample_organize.wav`(W5 既有 fixture,中文 "整理下载目录里的 PDF")
//! - 任一 fixture 缺失 → skip(返回 Ok)而非 fail
//! - sherpa-rs SenseVoice 模型缺失 → skip

#![cfg(feature = "voice")]

use std::path::PathBuf;
use trust_kernel::voice::asr::{SherpaAsrConfig, SherpaAsrEngine};
use trust_kernel::voice::model::{ModelRegistry, SENSE_VOICE_DIR_NAME};
use trust_kernel::voice::vad::{VadConfig, VadDetector};
use trust_kernel::voice::wav::read_wav;

/// 定位测试 fixture WAV 文件。优先 Plan 3 专用 fixture,回退到 W5 既有 fixture。
/// 若都不存在返回 None,测试 skip。
fn find_sample_wav() -> Option<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let candidates = [
        // Plan 3 专用 fixture(若用户提供)
        root.join("tests/fixtures/push_to_talk_sample.wav"),
        // W5 既有 fixture(中文 utterance,~3s,有语音内容)
        root.join("tests/fixtures/w5_sample_organize.wav"),
        root.join("tests/fixtures/w5_sample_yes.wav"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

/// 读取 WAV 文件为 16kHz mono i16 PCM 样本。使用 trust_kernel::voice::wav::read_wav。
fn read_wav_samples(path: &std::path::Path) -> Vec<i16> {
    let (samples, sample_rate) = read_wav(path).expect("failed to read WAV fixture");
    assert_eq!(
        sample_rate, 16000,
        "fixture must be 16kHz, got {}",
        sample_rate
    );
    samples
}

/// 定位 sherpa-rs SenseVoice 模型目录。若不存在返回 None,测试 skip。
///
/// 模型下载由 `voice model download` CLI 命令触发,默认存于
/// `%USERPROFILE%\.voicepilot\models\<SENSE_VOICE_DIR_NAME>`(Wave 0 冻结的
/// canonical 路径;旧 `%LOCALAPPDATA%\voicepilot\models` 由 ModelRegistry 兼容探测)。
/// 这里用 ModelRegistry::resolve 复用既有的解析逻辑。
fn resolve_sense_voice_model() -> Option<PathBuf> {
    let registry = ModelRegistry::new();
    registry.resolve(SENSE_VOICE_DIR_NAME).ok()
}

#[test]
#[ignore = "requires sherpa-rs SenseVoice model + WAV fixture; run with --ignored"]
fn p95_first_partial_transcript_under_500ms() {
    let wav_path = match find_sample_wav() {
        Some(p) => p,
        None => {
            eprintln!(
                "skip: no WAV fixture found (push_to_talk_sample.wav / w5_sample_organize.wav / w5_sample_yes.wav)"
            );
            return;
        }
    };
    let model_dir = match resolve_sense_voice_model() {
        Some(p) => p,
        None => {
            eprintln!("skip: SenseVoice model not found; run 'voice model download' first");
            return;
        }
    };

    let samples = read_wav_samples(&wav_path);
    assert!(!samples.is_empty(), "WAV fixture must contain samples");

    let engine = SherpaAsrEngine::new(SherpaAsrConfig {
        model_dir,
        language: Some("auto".to_string()),
        num_threads: 4,
        sample_rate: 16000,
    })
    .expect("failed to load sherpa-rs engine");

    // spec §5.2 v2 修订 #7:100 样本,P95 = 索引 94
    let mut latencies: Vec<u64> = Vec::with_capacity(100);

    // 模拟 VAD 检测首个 voiced chunk(t0)→ ASR 转写(t1)
    // 真实 voice loop 中,t0 由 VadDetector::chunk_has_speech 触发,
    // t1 由 partial transcript callback 触发。此处简化为同步转写整个 WAV,
    // 测量 engine.transcribe() 的 wall clock 时间作为 latency 近似。
    let vad = VadDetector::new(VadConfig::default());

    // 预先确认 WAV 中有 voiced chunk(避免 100 次循环都 skip)
    let chunk_size = 8000; // 500ms @ 16kHz
    let mut has_voiced = false;
    for chunk_start in (0..samples.len()).step_by(chunk_size) {
        let chunk_end = (chunk_start + chunk_size).min(samples.len());
        let chunk = &samples[chunk_start..chunk_end];
        if vad.chunk_has_speech(chunk) {
            has_voiced = true;
            break;
        }
    }
    assert!(
        has_voiced,
        "WAV fixture must contain at least one voiced chunk"
    );

    for i in 0..100 {
        // t0 = VAD 检测首个 voiced chunk 的时刻(此处用 transcribe 调用前的时间近似,
        // 因为 VAD 检测在真实 loop 中先于 ASR,此处测量 ASR 本身的延迟)
        let t0 = std::time::Instant::now();

        // ASR 转写(模拟 partial transcript 生成,t1)
        let transcript = engine.transcribe(&samples).expect("transcribe failed");
        let t1 = std::time::Instant::now();

        let latency_ms = t1.duration_since(t0).as_millis() as u64;
        latencies.push(latency_ms);

        // 确保转写非空(证明 ASR 实际工作)—— 仅首轮断言,避免 100 次 log 噪音
        if i == 0 {
            assert!(
                !transcript.trim().is_empty(),
                "transcript must be non-empty for valid WAV fixture (got: {:?})",
                transcript
            );
        }
    }

    assert_eq!(
        latencies.len(),
        100,
        "need 100 valid samples for P95, got {}",
        latencies.len()
    );

    latencies.sort_unstable();
    let p95_idx = ((latencies.len() as u64 * 95).div_ceil(100)) as usize;
    let p95 = latencies[p95_idx.saturating_sub(1).min(latencies.len() - 1)];

    let p50_idx = ((latencies.len() as u64 * 50).div_ceil(100)) as usize;
    let p50 = latencies[p50_idx.saturating_sub(1).min(latencies.len() - 1)];

    let p99_idx = ((latencies.len() as u64 * 99).div_ceil(100)) as usize;
    let p99 = latencies[p99_idx.saturating_sub(1).min(latencies.len() - 1)];

    eprintln!(
        "P95 benchmark: samples={}, P50={}ms, P95={}ms, P99={}ms, max={}ms",
        latencies.len(),
        p50,
        p95,
        p99,
        latencies.last().unwrap()
    );

    assert!(
        p95 <= 500,
        "P95 first partial transcript {}ms > 500ms (spec VP-FR-001)",
        p95
    );
}
