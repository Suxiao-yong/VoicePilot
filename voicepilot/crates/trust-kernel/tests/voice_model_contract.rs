#![cfg(feature = "voice")]

use trust_kernel::voice::model_download::default_model_info;

#[test]
fn default_model_info_pins_sensevoice_archive_contract() {
    let info = default_model_info();

    assert_eq!(
        info.name,
        "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17"
    );
    assert_eq!(info.archive_size_bytes, 1_047_870_769);
    assert_eq!(info.size_hint_mb, 1048);
    assert_eq!(
        info.expected_sha256.as_deref(),
        Some("f6b2a72ebcb1ac7a764d4cfccd886e6bcb2a95c4657c2199d0ba95ed4b9ea71a")
    );
    assert_eq!(
        info.download_url.as_str(),
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17.tar.bz2"
    );
}
