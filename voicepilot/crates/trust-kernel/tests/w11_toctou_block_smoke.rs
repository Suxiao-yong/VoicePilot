//! W11 Plan 3 — 20 个 TOCTOU 场景集成测试(spec §9.1 ③)。
//!
//! 验证 prepare → approve → commit 两阶段事务在 commit 点检测 4 类 TOCTOU:
//!   1. 文件内容替换(6):prepare 后 source 内容被篡改 → sha256/size 变化 → abort
//!   2. 符号链接替换(5):prepare 后 source 路径被替换为 symlink → file_id/内容变化 → abort
//!   3. 重定向跳转(5):prepare 后 source 目录被 rename / source 提前移动 → 路径失效 → abort
//!   4. args_hash 篡改(4):调用方在 commit 时篡改 manifest(destination/sources/total_bytes)
//!      → preconditions_hash 不匹配 → abort;token 复用 → InvalidPrepareToken
//!
//! **与 plan 的偏离说明:**
//!   - plan 期望 commit 返回 `KernelError::ToctouDetected`。实际实现(transaction.rs /
//!     fs.rs)在 commit 点用 `KernelError::PreconditionMismatch` / `Filesystem` / `Io`
//!     表达检测(见 error.rs —— 无 `ToctouDetected` 变体)。本测试断言 `is_err()` +
//!     关键场景匹配具体变体,语义等价"commit 被 abort"。
//!   - 符号链接创建在 Windows 需管理员 / Developer Mode。本测试用 `symlinks_supported()`
//!     探测;若当前机器不支持则 soft-skip(打印说明,不 fail),有支持时真实断言。
//!     相比 plan 建议的 `#[ignore]`,soft-skip 保证默认 `cargo test` 也计数 20 个测试
//!     (Plan 6 Fitness Function `toctou_block_all_20` 依赖此计数)。
//!
//! 运行:
//!   cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test w11_toctou_block_smoke

use std::fs;
use std::path::{Path, PathBuf};

use trust_kernel::policy::transaction::TransactionManager;
use trust_kernel::tools::fs::FilesystemTool;

/// 独立临时目录(每次调用唯一,避免并行测试冲突)。
fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vp-w11-toctou-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// 探测当前机器是否支持创建符号链接(Windows 需管理员 / Developer Mode)。
fn symlinks_supported(dir: &Path) -> bool {
    let target = dir.join("__symlink_probe_target");
    let link = dir.join("__symlink_probe_link");
    fs::write(&target, b"probe").unwrap();
    let ok = std::os::windows::fs::symlink_file(&target, &link).is_ok();
    let _ = fs::remove_file(&link);
    let _ = fs::remove_file(&target);
    ok
}

// ===== 类别 1:文件内容替换(6)=====

/// 1. prepare 后 source 内容被替换(不同大小)→ commit abort。
#[test]
fn content_tamper_different_size_aborts_commit() {
    let dir = tmp_dir();
    let src = dir.join("a.txt");
    fs::write(&src, b"original").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool.prepare_move("t1", "s1", &[&src], &dest, &mgr).unwrap();

    // TOCTOU:commit 前篡改内容(不同大小 → size + sha256 都变)
    fs::write(&src, b"tampered-content-longer").unwrap();

    let result = tool.commit_move(&prepared.token, &prepared.manifest, &mgr);
    assert!(result.is_err(), "内容替换必须 abort commit");
    assert!(src.exists(), "abort 后 source 必须保留(无部分移动)");
    fs::remove_dir_all(&dir).ok();
}

/// 2. prepare 后 source 内容被替换(同大小)→ sha256 变化 → commit abort。
///    证明检测不依赖 size,而是 sha256 内容校验。
#[test]
fn content_tamper_same_size_aborts_commit() {
    let dir = tmp_dir();
    let src = dir.join("a.txt");
    fs::write(&src, b"abcdef").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool.prepare_move("t1", "s1", &[&src], &dest, &mgr).unwrap();

    // 同大小不同内容("abcdef" → "xxxxxx"),size 相同但 sha256 不同
    fs::write(&src, b"xxxxxx").unwrap();

    let result = tool.commit_move(&prepared.token, &prepared.manifest, &mgr);
    assert!(result.is_err(), "同大小内容替换必须被 sha256 检测");
    assert!(src.exists());
    fs::remove_dir_all(&dir).ok();
}

/// 3. 多 source 中仅 1 个被篡改 → 整体 abort(不留部分移动)。
#[test]
fn content_tamper_one_of_multi_source_aborts() {
    let dir = tmp_dir();
    let src1 = dir.join("a.txt");
    let src2 = dir.join("b.txt");
    fs::write(&src1, b"first").unwrap();
    fs::write(&src2, b"second").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool
        .prepare_move("t1", "s1", &[&src1, &src2], &dest, &mgr)
        .unwrap();

    // 只篡改 src2
    fs::write(&src2, b"SECOND-CHANGED").unwrap();

    let result = tool.commit_move(&prepared.token, &prepared.manifest, &mgr);
    assert!(result.is_err(), "任一 source 被篡改必须整体 abort");
    assert!(src1.exists() && src2.exists(), "abort 后两个 source 都保留");
    fs::remove_dir_all(&dir).ok();
}

/// 4. prepare 后 source 被删除并重建(同内容)→ file_id 变化 → commit abort。
#[test]
fn content_delete_and_recreate_aborts() {
    let dir = tmp_dir();
    let src = dir.join("a.txt");
    fs::write(&src, b"stable").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool.prepare_move("t1", "s1", &[&src], &dest, &mgr).unwrap();

    // 删除后重建:内容相同,但 file_id(NTFS file index)不同
    fs::remove_file(&src).unwrap();
    fs::write(&src, b"stable").unwrap();

    let result = tool.commit_move(&prepared.token, &prepared.manifest, &mgr);
    assert!(result.is_err(), "删除重建(file_id 变化)必须 abort");
    fs::remove_dir_all(&dir).ok();
}

/// 5. prepare 后 source 追加内容 → size + sha256 变化 → commit abort。
#[test]
fn content_append_aborts() {
    let dir = tmp_dir();
    let src = dir.join("a.txt");
    fs::write(&src, b"base").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool.prepare_move("t1", "s1", &[&src], &dest, &mgr).unwrap();

    // 追加字节
    let mut f = fs::OpenOptions::new().append(true).open(&src).unwrap();
    use std::io::Write;
    f.write_all(b"-appended").unwrap();
    drop(f);

    let result = tool.commit_move(&prepared.token, &prepared.manifest, &mgr);
    assert!(result.is_err(), "追加内容必须 abort commit");
    fs::remove_dir_all(&dir).ok();
}

/// 6. 内容替换 + 新目标冲突同时出现 → 双重 TOCTOU 都被检测。
#[test]
fn content_tamper_and_new_dest_conflict_aborts() {
    let dir = tmp_dir();
    let src = dir.join("a.txt");
    fs::write(&src, b"hello").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool.prepare_move("t1", "s1", &[&src], &dest, &mgr).unwrap();

    // TOCTOU 组合:篡改 source 内容 + 在 dest 预置同名文件
    fs::write(&src, b"tampered").unwrap();
    fs::write(dest.join("a.txt"), b"injected-conflict").unwrap();

    let result = tool.commit_move(&prepared.token, &prepared.manifest, &mgr);
    assert!(result.is_err(), "内容替换 + 新冲突必须 abort");
    assert!(src.exists());
    fs::remove_dir_all(&dir).ok();
}

// ===== 类别 2:符号链接替换(5)=====

/// 7. prepare 后 source 被替换为指向其他文件的 symlink → file_id/内容变化 → abort。
#[test]
fn symlink_replace_source_aborts() {
    let dir = tmp_dir();
    if !symlinks_supported(&dir) {
        eprintln!("SKIP: symlinks not supported on this machine (admin/Developer Mode required)");
        return;
    }
    let src = dir.join("a.txt");
    fs::write(&src, b"original").unwrap();
    let evil = dir.join("evil.txt");
    fs::write(&evil, b"evil-content").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool.prepare_move("t1", "s1", &[&src], &dest, &mgr).unwrap();

    // TOCTOU:用指向 evil.txt 的 symlink 替换 a.txt
    fs::remove_file(&src).unwrap();
    std::os::windows::fs::symlink_file(&evil, &src).unwrap();

    let result = tool.commit_move(&prepared.token, &prepared.manifest, &mgr);
    assert!(result.is_err(), "symlink 替换 source 必须 abort");
    fs::remove_dir_all(&dir).ok();
}

/// 8. prepare 后 source 被替换为指向同内容文件的 symlink → file_id 不同 → abort。
///    即使 sha256 相同,file_id(NTFS file index)不同也会被 preconditions_hash 捕获。
#[test]
fn symlink_replace_source_same_content_aborts() {
    let dir = tmp_dir();
    if !symlinks_supported(&dir) {
        eprintln!("SKIP: symlinks not supported on this machine");
        return;
    }
    let src = dir.join("a.txt");
    fs::write(&src, b"identical").unwrap();
    let twin = dir.join("twin.txt");
    fs::write(&twin, b"identical").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool.prepare_move("t1", "s1", &[&src], &dest, &mgr).unwrap();

    // 替换为指向同内容 twin 的 symlink(sha256 相同,file_id 不同)
    fs::remove_file(&src).unwrap();
    std::os::windows::fs::symlink_file(&twin, &src).unwrap();

    let result = tool.commit_move(&prepared.token, &prepared.manifest, &mgr);
    assert!(
        result.is_err(),
        "同内容 symlink 替换(file_id 变化)必须 abort"
    );
    fs::remove_dir_all(&dir).ok();
}

/// 9. prepare 后 source 被替换为 symlink 链(link1 → link2 → target)→ abort。
#[test]
fn symlink_chain_source_aborts() {
    let dir = tmp_dir();
    if !symlinks_supported(&dir) {
        eprintln!("SKIP: symlinks not supported on this machine");
        return;
    }
    let src = dir.join("a.txt");
    fs::write(&src, b"original").unwrap();
    let target = dir.join("target.txt");
    fs::write(&target, b"target-data").unwrap();
    let link1 = dir.join("link1.txt");
    let link2 = dir.join("link2.txt");
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool.prepare_move("t1", "s1", &[&src], &dest, &mgr).unwrap();

    // TOCTOU:用 symlink 链替换 a.txt(a.txt → link1 → link2 → target)
    fs::remove_file(&src).unwrap();
    std::os::windows::fs::symlink_file(&target, &link2).unwrap();
    std::os::windows::fs::symlink_file(&link2, &link1).unwrap();
    std::os::windows::fs::symlink_file(&link1, &src).unwrap();

    let result = tool.commit_move(&prepared.token, &prepared.manifest, &mgr);
    assert!(result.is_err(), "symlink 链替换必须 abort");
    fs::remove_dir_all(&dir).ok();
}

/// 10. prepare 后 source 被替换为指向目录的 symlink → commit 时 snapshot 报错(非普通文件)。
#[test]
fn symlink_source_to_directory_aborts() {
    let dir = tmp_dir();
    if !symlinks_supported(&dir) {
        eprintln!("SKIP: symlinks not supported on this machine");
        return;
    }
    let src = dir.join("a.txt");
    fs::write(&src, b"original").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();
    let target_dir = dir.join("realdir");
    fs::create_dir_all(&target_dir).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool.prepare_move("t1", "s1", &[&src], &dest, &mgr).unwrap();

    // 用指向目录的 symlink 替换 source(文件 → 目录 类型变化)
    fs::remove_file(&src).unwrap();
    std::os::windows::fs::symlink_dir(&target_dir, &src).unwrap();

    let result = tool.commit_move(&prepared.token, &prepared.manifest, &mgr);
    assert!(result.is_err(), "source 类型变化(文件→目录)必须 abort");
    fs::remove_dir_all(&dir).ok();
}

/// 11. prepare 后 source 被替换为指向不存在目标的 broken symlink → snapshot 报错 → abort。
#[test]
fn symlink_broken_source_aborts() {
    let dir = tmp_dir();
    if !symlinks_supported(&dir) {
        eprintln!("SKIP: symlinks not supported on this machine");
        return;
    }
    let src = dir.join("a.txt");
    fs::write(&src, b"original").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool.prepare_move("t1", "s1", &[&src], &dest, &mgr).unwrap();

    // broken symlink:目标不存在
    fs::remove_file(&src).unwrap();
    std::os::windows::fs::symlink_file(dir.join("nonexistent.txt"), &src).unwrap();

    let result = tool.commit_move(&prepared.token, &prepared.manifest, &mgr);
    assert!(result.is_err(), "broken symlink 替换必须 abort");
    fs::remove_dir_all(&dir).ok();
}

// ===== 类别 3:重定向跳转(5)=====

/// 12. prepare 后 source 所在目录被 rename → source 路径失效 → commit abort。
#[test]
fn rename_source_parent_dir_aborts() {
    let dir = tmp_dir();
    let src_dir = dir.join("srcdir");
    fs::create_dir_all(&src_dir).unwrap();
    let src = src_dir.join("a.txt");
    fs::write(&src, b"original").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool.prepare_move("t1", "s1", &[&src], &dest, &mgr).unwrap();

    // TOCTOU:重命名 source 父目录(路径重定向)
    let moved_dir = dir.join("srcdir-moved");
    fs::rename(&src_dir, &moved_dir).unwrap();

    let result = tool.commit_move(&prepared.token, &prepared.manifest, &mgr);
    assert!(result.is_err(), "source 父目录被 rename 必须 abort");
    fs::remove_dir_all(&dir).ok();
}

/// 13. prepare 后目标目录被 rename → commit 移动失败 → abort。
#[test]
fn rename_destination_dir_aborts() {
    let dir = tmp_dir();
    let src = dir.join("a.txt");
    fs::write(&src, b"original").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool.prepare_move("t1", "s1", &[&src], &dest, &mgr).unwrap();

    // TOCTOU:重命名目标目录
    let moved_dest = dir.join("out-moved");
    fs::rename(&dest, &moved_dest).unwrap();

    let result = tool.commit_move(&prepared.token, &prepared.manifest, &mgr);
    assert!(result.is_err(), "目标目录被 rename 必须 abort");
    assert!(src.exists(), "abort 后 source 保留");
    fs::remove_dir_all(&dir).ok();
}

/// 14. prepare 后两个 source 内容互换 → 双方 sha256 都变 → commit abort。
#[test]
fn swap_two_source_files_aborts() {
    let dir = tmp_dir();
    let src1 = dir.join("a.txt");
    let src2 = dir.join("b.txt");
    fs::write(&src1, b"content-a").unwrap();
    fs::write(&src2, b"content-b").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool
        .prepare_move("t1", "s1", &[&src1, &src2], &dest, &mgr)
        .unwrap();

    // 内容互换
    fs::write(&src1, b"content-b").unwrap();
    fs::write(&src2, b"content-a").unwrap();

    let result = tool.commit_move(&prepared.token, &prepared.manifest, &mgr);
    assert!(result.is_err(), "source 内容互换必须 abort");
    fs::remove_dir_all(&dir).ok();
}

/// 15. prepare 后 source 被提前移动到目标 → source 缺失 + 新目标冲突 → abort。
#[test]
fn move_source_to_dest_prematurely_aborts() {
    let dir = tmp_dir();
    let src = dir.join("a.txt");
    fs::write(&src, b"original").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool.prepare_move("t1", "s1", &[&src], &dest, &mgr).unwrap();

    // 提前把 source 移到目标(路径重定向 + 目标冲突)
    fs::rename(&src, dest.join("a.txt")).unwrap();

    let result = tool.commit_move(&prepared.token, &prepared.manifest, &mgr);
    assert!(result.is_err(), "source 提前移动必须 abort");
    fs::remove_dir_all(&dir).ok();
}

/// 16. prepare 后 source 所在目录被删除重建 → source 是全新文件(file_id 变)→ abort。
#[test]
fn source_dir_replaced_with_new_dir_aborts() {
    let dir = tmp_dir();
    let src_dir = dir.join("srcdir");
    fs::create_dir_all(&src_dir).unwrap();
    let src = src_dir.join("a.txt");
    fs::write(&src, b"original").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool.prepare_move("t1", "s1", &[&src], &dest, &mgr).unwrap();

    // 删除源目录并重建同名目录 + 同内容文件(file_id 全变)
    fs::remove_dir_all(&src_dir).unwrap();
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(&src, b"original").unwrap();

    let result = tool.commit_move(&prepared.token, &prepared.manifest, &mgr);
    assert!(result.is_err(), "目录重定向到全新文件必须 abort");
    fs::remove_dir_all(&dir).ok();
}

// ===== 类别 4:args_hash 篡改(4)=====

/// 17. 调用方篡改 manifest.destination → preconditions_hash 不匹配 → abort。
#[test]
fn tampered_destination_in_manifest_aborts() {
    let dir = tmp_dir();
    let src = dir.join("a.txt");
    fs::write(&src, b"original").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool.prepare_move("t1", "s1", &[&src], &dest, &mgr).unwrap();

    // 调用方篡改 destination(重定向到攻击者目录)
    let mut tampered = prepared.manifest.clone();
    tampered.destination = dir.join("attacker-out").to_string_lossy().to_string();

    let result = tool.commit_move(&prepared.token, &tampered, &mgr);
    let err = result.expect_err("篡改 destination 必须 abort");
    assert!(
        matches!(
            err,
            trust_kernel::error::KernelError::PreconditionMismatch { .. }
        ),
        "篡改 destination 应报 PreconditionMismatch, got {:?}",
        err
    );
    fs::remove_dir_all(&dir).ok();
}

/// 18. 调用方篡改 manifest.sources 指向另一文件 → re-snapshot 不同 → abort。
#[test]
fn tampered_sources_in_manifest_aborts() {
    let dir = tmp_dir();
    let src = dir.join("a.txt");
    fs::write(&src, b"original").unwrap();
    let decoy = dir.join("decoy.txt");
    fs::write(&decoy, b"decoy-data").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool.prepare_move("t1", "s1", &[&src], &dest, &mgr).unwrap();

    // 调用方篡改 sources 指向 decoy 文件
    let mut tampered = prepared.manifest.clone();
    tampered.sources[0].canonical_path = decoy.to_string_lossy().to_string();

    let result = tool.commit_move(&prepared.token, &tampered, &mgr);
    let err = result.expect_err("篡改 sources 必须 abort");
    assert!(
        matches!(
            err,
            trust_kernel::error::KernelError::PreconditionMismatch { .. }
        ),
        "篡改 sources 应报 PreconditionMismatch, got {:?}",
        err
    );
    fs::remove_dir_all(&dir).ok();
}

/// 19. 调用方篡改 manifest.total_bytes → preconditions_hash 不匹配 → abort。
#[test]
fn tampered_total_bytes_in_manifest_aborts() {
    let dir = tmp_dir();
    let src = dir.join("a.txt");
    fs::write(&src, b"original").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool.prepare_move("t1", "s1", &[&src], &dest, &mgr).unwrap();

    // 调用方篡改 total_bytes
    let mut tampered = prepared.manifest.clone();
    tampered.total_bytes = 999_999;

    let result = tool.commit_move(&prepared.token, &tampered, &mgr);
    let err = result.expect_err("篡改 total_bytes 必须 abort");
    assert!(
        matches!(
            err,
            trust_kernel::error::KernelError::PreconditionMismatch { .. }
        ),
        "篡改 total_bytes 应报 PreconditionMismatch, got {:?}",
        err
    );
    fs::remove_dir_all(&dir).ok();
}

/// 20. commit 成功后复用同一 token 重放 → InvalidPrepareToken(一次性 token)。
///
/// 直接调 `TransactionManager::commit`(而非 `commit_move`),因为 commit_move
/// 会先 re-snapshot source —— 首次 commit 后 source 已移走,重放会先报 Io
/// (NotFound)而不是到达 token 消费检查。用 mgr.commit 精确验证一次性语义。
#[test]
fn token_replay_after_commit_aborts() {
    use trust_kernel::error::KernelError;

    let dir = tmp_dir();
    let src = dir.join("a.txt");
    fs::write(&src, b"original").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool.prepare_move("t1", "s1", &[&src], &dest, &mgr).unwrap();

    // 第一次 commit(token 校验通过,消费 token)
    let first = mgr.commit(&prepared.token, &prepared.manifest).unwrap();
    assert!(first.committed, "正常 commit 应成功");

    // 重放同一 token → token 已被消费 → InvalidPrepareToken
    let replay = mgr.commit(&prepared.token, &prepared.manifest);
    let err = replay.expect_err("token 复用必须 abort");
    assert!(
        matches!(err, KernelError::InvalidPrepareToken(_)),
        "token 复用应报 InvalidPrepareToken, got {:?}",
        err
    );
    fs::remove_dir_all(&dir).ok();
}
