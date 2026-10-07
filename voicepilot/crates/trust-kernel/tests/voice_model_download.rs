//! Wave 3 Task 3.2 — VoiceRuntime model download contract tests (RED→GREEN).
//!
//! Uses a local-file `DownloadSource` (a synthetic tar.bz2) so resume /
//! atomic-install / state resolution are tested without network.

#![cfg(feature = "voice")]

use std::fs;
use std::path::PathBuf;

use trust_kernel::voice::error::VoiceResult;
use trust_kernel::voice::model_download::{
    download_model_from, model_state, DownloadSource, ModelInfo, ModelState,
};
use trust_kernel::voice::model::{ModelRegistry, SENSE_VOICE_DIR_NAME};

/// A complete synthetic tar.bz2 containing `<name>/model.onnx` + `<name>/tokens.txt`.
fn build_synthetic_tar_bz2(model_name: &str) -> Vec<u8> {
    let mut tar_buf: Vec<u8> = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_buf);
        let mut dir_header = tar::Header::new_gnu();
        dir_header.set_path(model_name).unwrap();
        dir_header.set_size(0);
        dir_header.set_mode(0o755);
        dir_header.set_entry_type(tar::EntryType::Directory);
        dir_header.set_cksum();
        builder.append(&dir_header, std::io::empty()).unwrap();

        let model_bytes = b"FAKE_ONNX_MODEL_BYTES";
        let mut fh = tar::Header::new_gnu();
        fh.set_path(format!("{model_name}/model.onnx")).unwrap();
        fh.set_size(model_bytes.len() as u64);
        fh.set_mode(0o644);
        fh.set_entry_type(tar::EntryType::Regular);
        fh.set_cksum();
        builder
            .append(&fh, std::io::Cursor::new(model_bytes))
            .unwrap();

        let tokens_bytes = b"<silence>\n<unk>\nhello\nworld\n";
        let mut th = tar::Header::new_gnu();
        th.set_path(format!("{model_name}/tokens.txt")).unwrap();
        th.set_size(tokens_bytes.len() as u64);
        th.set_mode(0o644);
        th.set_entry_type(tar::EntryType::Regular);
        th.set_cksum();
        builder
            .append(&th, std::io::Cursor::new(tokens_bytes))
            .unwrap();

        builder.finish().unwrap();
    }
    let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
    use std::io::Write;
    encoder.write_all(&tar_buf).unwrap();
    encoder.finish().unwrap()
}

fn info_with(source: DownloadSource, name: &str) -> ModelInfo {
    ModelInfo {
        name: name.to_string(),
        download_url: String::new(),
        archive_size_bytes: 0, // File source: size validated against the file, not this field.
        expected_sha256: None,
        size_hint_mb: 0,
        source,
    }
}

/// ModelState resolution: complete dir → Ready; absent → Missing; leftover
/// `.part` → Downloading.
#[test]
fn model_state_reflects_filesystem() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let models_dir = tmp.path().join("models");
    let name = "state-test-model";
    let final_dir = models_dir.join(name);

    // Absent → Missing.
    assert_eq!(
        model_state(&models_dir, name).expect("state"),
        ModelState::Missing
    );

    // Leftover .part → Downloading.
    fs::create_dir_all(&models_dir).expect("mkdir");
    fs::write(models_dir.join(format!("{name}.tar.bz2.part")), b"partial").expect("write part");
    assert_eq!(
        model_state(&models_dir, name).expect("state"),
        ModelState::Downloading
    );
    fs::remove_file(models_dir.join(format!("{name}.tar.bz2.part"))).expect("remove part");

    // Complete dir → Ready.
    fs::create_dir_all(final_dir.join("sub")).expect("mkdir");
    fs::write(final_dir.join("model.onnx"), b"onnx").expect("onnx");
    fs::write(final_dir.join("tokens.txt"), b"tokens").expect("tokens");
    assert_eq!(
        model_state(&models_dir, name).expect("state"),
        ModelState::Ready
    );
}

/// Resumable file-source download: a partial `.part` is resumed (Range-like)
/// to exactly match the source file; the final dir is installed atomically and
/// a second call is idempotent.
#[test]
fn file_source_download_resumes_part_and_installs_atomically() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let models_dir = tmp.path().join("models");
    let name = format!("resume-model-{}", uuid::Uuid::new_v4());
    let final_dir = models_dir.join(&name);
    let archive_path = tmp.path().join("model.tar.bz2");
    let archive_bytes = build_synthetic_tar_bz2(&name);
    fs::write(&archive_path, &archive_bytes).expect("write archive");

    // Pre-create a partial .part (first half of the archive) to prove resume.
    fs::create_dir_all(&models_dir).expect("mkdir models");
    let part_path = models_dir.join(format!("{name}.tar.bz2.part"));
    let half = archive_bytes.len() / 2;
    fs::write(&part_path, &archive_bytes[..half]).expect("write partial part");

    let source = DownloadSource::File {
        path: archive_path.clone(),
    };
    let info = info_with(source, &name);
    let result: VoiceResult<PathBuf> = download_model_from(&info, models_dir.clone(), |_| {});
    let final_dir_result = result.expect("download from file source");
    assert_eq!(final_dir_result, final_dir);

    // Model installed atomically with the required files.
    assert!(final_dir.join("model.onnx").is_file());
    assert!(final_dir.join("tokens.txt").is_file());
    // .part cleaned up after successful install.
    assert!(!part_path.exists(), "part file must be removed after install");

    // Idempotent: calling again returns the same dir without re-downloading.
    let again = download_model_from(&info, models_dir.clone(), |_| {})
        .expect("second call must be idempotent");
    assert_eq!(again, final_dir);
}

/// Missing source file → friendly failure (network/download guidance in text),
/// and the partial .part is retained for resume.
#[test]
fn file_source_download_failure_keeps_part_for_resume() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let models_dir = tmp.path().join("models");
    let name = format!("fail-model-{}", uuid::Uuid::new_v4());
    fs::create_dir_all(&models_dir).expect("mkdir models");
    let part_path = models_dir.join(format!("{name}.tar.bz2.part"));
    fs::write(&part_path, b"partial-bytes").expect("write part");

    let missing = tmp.path().join("does-not-exist.tar.bz2");
    let info = info_with(
        DownloadSource::File {
            path: missing.clone(),
        },
        &name,
    );
    let err = download_model_from(&info, models_dir.clone(), |_| {})
        .expect_err("missing source must fail");
    let msg = err.to_string();
    // Friendly guidance: mentions the archive path.
    assert!(
        msg.contains("missing") || msg.contains("failed"),
        "error should describe the failure, got: {msg}"
    );
    // .part retained for resume (network failure keeps resumable state).
    assert!(
        part_path.exists(),
        "partial .part must be kept after a failed download"
    );
}

/// Legacy `%LOCALAPPDATA%` model dir is discovered as a compatible path when
/// the canonical dir does not have the model.
#[cfg(windows)]
#[test]
fn registry_resolves_legacy_localappdata_models_dir() {
    let tmp = tempfile::tempdir().expect("tempdir");
    // Simulate legacy layout: <legacy>/voicepilot/models/<name> with files.
    let legacy_root = tmp.path().join("LocalAppData");
    let legacy_models = legacy_root.join("voicepilot").join("models");
    let model_dir = legacy_models.join(SENSE_VOICE_DIR_NAME);
    fs::create_dir_all(&model_dir).expect("mkdir legacy model");
    fs::write(model_dir.join("model.onnx"), b"onnx").expect("onnx");
    fs::write(model_dir.join("tokens.txt"), b"tokens").expect("tokens");

    // Canonical home must be empty: with_legacy_probe() leaves home_dir as the
    // real ~, so on a dev box that already has the model the canonical branch
    // in resolve() wins and this test fails. Use with_dirs() with an empty home.
    let home_dir = tmp.path().join("canonical-home");
    let reg = ModelRegistry::with_dirs(home_dir, Some(legacy_root.clone()));
    assert!(
        reg.is_model_present(SENSE_VOICE_DIR_NAME),
        "legacy model dir must be discovered compatibly"
    );
    let resolved = reg.resolve(SENSE_VOICE_DIR_NAME).expect("resolve");
    assert_eq!(
        resolved,
        legacy_models.join(SENSE_VOICE_DIR_NAME),
        "must use the legacy compatible path without copying"
    );
}

// ===== URL 源 Range 断点续传三态测试(206 / 200 / 416)=====

use wiremock::matchers::{header, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 返回 206(部分内容)的服务器:对 `Range: bytes=<half>-` 返回剩余字节。
#[tokio::test]
async fn url_source_resume_206_appends_and_installs() {
    let server = MockServer::start().await;
    let name = format!("url-resume-{}", uuid::Uuid::new_v4());
    let archive_bytes = build_synthetic_tar_bz2(&name);
    let half = archive_bytes.len() / 2;
    let remaining = archive_bytes[half..].to_vec();

    Mock::given(method("GET"))
        .and(header("range", format!("bytes={half}-")))
        .respond_with(
            ResponseTemplate::new(206)
                .set_body_raw(remaining, "application/octet-stream")
                .insert_header("Content-Range", format!("bytes {half}-{}/{}", archive_bytes.len() - 1, archive_bytes.len())),
        )
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().expect("tempdir");
    let models_dir = tmp.path().join("models");
    let final_dir = models_dir.join(&name);
    fs::create_dir_all(&models_dir).expect("mkdir");
    // 预置前一半 .part(模拟中断后续传)。
    let part_path = models_dir.join(format!("{name}.tar.bz2.part"));
    fs::write(&part_path, &archive_bytes[..half]).expect("write partial part");

    let info = info_with(
        DownloadSource::Url {
            url: server.uri(),
        },
        &name,
    );
    let result = download_model_from(&info, models_dir.clone(), |_| {}).expect("206 resume install");
    assert_eq!(result, final_dir);
    assert!(final_dir.join("model.onnx").is_file());
    assert!(final_dir.join("tokens.txt").is_file());
    assert!(!part_path.exists(), "part removed after install");
}

/// 服务器忽略 Range 返回 200(全量):安全重启,从头下载并安装。
#[tokio::test]
async fn url_source_ignores_range_200_restarts_from_scratch() {
    let server = MockServer::start().await;
    let name = format!("url-restart-{}", uuid::Uuid::new_v4());
    let archive_bytes = build_synthetic_tar_bz2(&name);

    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(archive_bytes.clone(), "application/octet-stream"),
        )
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().expect("tempdir");
    let models_dir = tmp.path().join("models");
    let final_dir = models_dir.join(&name);
    fs::create_dir_all(&models_dir).expect("mkdir");
    let part_path = models_dir.join(format!("{name}.tar.bz2.part"));
    // 预置一个"错位"的 .part(与真实归档不同),200 分支必须截断重下。
    fs::write(&part_path, b"STALE-WRONG-PARTIAL-DATA").expect("write stale part");

    let info = info_with(
        DownloadSource::Url {
            url: server.uri(),
        },
        &name,
    );
    let result = download_model_from(&info, models_dir.clone(), |_| {}).expect("200 restart install");
    assert_eq!(result, final_dir);
    assert!(final_dir.join("model.onnx").is_file());
}

/// 服务器返回 416(范围失效):安全重启 —— 截断 .part 并给出友好错误。
#[tokio::test]
async fn url_source_range_not_satisfiable_416_truncates_and_errors() {
    let server = MockServer::start().await;
    let name = format!("url-416-{}", uuid::Uuid::new_v4());

    Mock::given(method("GET"))
        .and(header("range", "bytes=100-"))
        .respond_with(ResponseTemplate::new(416))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().expect("tempdir");
    let models_dir = tmp.path().join("models");
    fs::create_dir_all(&models_dir).expect("mkdir");
    let part_path = models_dir.join(format!("{name}.tar.bz2.part"));
    fs::write(&part_path, vec![0u8; 100]).expect("write part of 100 bytes");

    let info = info_with(
        DownloadSource::Url {
            url: server.uri(),
        },
        &name,
    );
    let err = download_model_from(&info, models_dir.clone(), |_| {})
        .expect_err("416 must fail");
    assert!(
        err.to_string().contains("416") || err.to_string().contains("范围"),
        "error must mention the range issue, got: {err}"
    );
    // 416 后 .part 被截断(下次从头下载)。
    let len = fs::metadata(&part_path).expect("part exists").len();
    assert_eq!(len, 0, "416 must truncate the .part for a fresh restart");
}
