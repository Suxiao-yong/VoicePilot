#![cfg(feature = "voice")]

//! Wave 5 Task 5.1 release-gate:CLI `voice list-models` 回归测试。
//!
//! 验证修复后的命令:
//! - 使用冻结的 SenseVoice 目录契约(model.onnx + tokens.txt 完整性),而非
//!   旧的 `path.is_file()`(目录模型恒为 [missing] 的 bug)。
//! - 文案不再出现 Whisper 字眼,正确指向 sherpa-onnx SenseVoice 与 .tar.bz2。
//! - 在全新环境(非存在的 USERPROFILE)下报告 [missing]。

use std::io::Write;
use std::process::{Command, Stdio};

fn voicepilot_bin() -> std::path::PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    std::path::Path::new(&manifest_dir)
        .join("..")
        .join("..")
        .join("target")
        .join("debug")
        .join("voicepilot.exe")
}

#[test]
fn voice_list_models_uses_sense_voice_contract() {
    // 用不存在的 USERPROFILE + HOME 保证模型目录不存在 → [missing] 是确定性的
    // (ModelRegistry::dirs_or_fallback 优先读 HOME,再读 USERPROFILE)。
    let fake_profile = std::path::Path::new("E:/definitely_nonexistent_voicepilot_profile");
    let mut child = Command::new(voicepilot_bin())
        .env("VOICEPILOT_DB", ":memory:")
        .env("USERPROFILE", fake_profile)
        .env("HOME", fake_profile)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn voicepilot");

    // CLI 是 REPL 风格:`voice list-models` 从 stdin 读取;quit 结束。
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"voice list-models\nquit\n")
        .expect("write command to stdin");

    let output = child.wait_with_output().expect("wait for voicepilot");

    assert!(
        output.status.success(),
        "voice list-models must exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("SenseVoice"),
        "must advertise SenseVoice, got: {stdout}"
    );
    assert!(
        !stdout.contains("Whisper"),
        "must not mention Whisper, got: {stdout}"
    );
    assert!(
        stdout.contains("model.onnx") && stdout.contains("tokens.txt"),
        "must reference the onnx/tokens completeness contract, got: {stdout}"
    );
    assert!(
        stdout.contains("[missing]"),
        "fresh profile must report the model as missing, got: {stdout}"
    );
    assert!(
        stdout.contains(".tar.bz2"),
        "install hint must reference the .tar.bz2 archive, got: {stdout}"
    );
}
