//! Daisy 移植：文件组（读/写/建/删/列）。
//!
//! 路径约定：绝对路径直用；`Documents/…`/`Desktop/…` 前缀解析到系统已知
//! 文件夹（`dirs` 已是依赖）；其余相对路径按进程 CWD（与 research.save
//! 一致）。写/建/删 PerStep 审批；写覆盖前留 `.vp-bak`、删除改可恢复
//! 改名（`.vp-deleted-<ts>`），输出里写明恢复位置。读/列只读免审批。

use crate::approval::approver::Approver;
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::skills::manifest::{EgressKind, SkillInputType, SkillManifest};
use crate::skills::simple::{run_simple, simple_manifest, slot_text, truncate_chars, SimpleInput};
use std::collections::HashMap;
use std::path::PathBuf;

fn file_slot(name: &'static str) -> SimpleInput {
    // allowed_roots 为空 = 不做前缀限制（validate 只在非空时 enforcing）；
    // 写操作的安全边界是 PerStep 审批 + 备份/可恢复语义，见各 execute。
    SimpleInput {
        name,
        input_type: SkillInputType::File,
        required: true,
        max_length: Some(500),
        allowed_roots: vec![],
    }
}

/// 用户路径解析：绝对直用；Documents/Desktop 前缀进已知文件夹。
pub fn resolve_user_path(raw: &str) -> PathBuf {
    let p = PathBuf::from(raw.trim());
    if p.is_absolute() {
        return p;
    }
    let s = raw.trim().replace('\\', "/");
    for (prefix, dir) in [
        ("Documents/", dirs::document_dir()),
        ("Desktop/", dirs::desktop_dir()),
    ] {
        if s.len() > prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
            if let Some(base) = dir {
                return base.join(&s[prefix.len()..]);
            }
        }
    }
    p
}

pub fn fs_read_manifest() -> SkillManifest {
    simple_manifest(
        "fs.read_file",
        "读文件",
        "读取文本文件内容（只读，超长截断）。",
        &["读文件", "看文件", "文件内容", "打开文件看"],
        &["读一下这个文件", "看看文件里写了什么"],
        vec![file_slot("path")],
        false,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_fs_read(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let path = slot_text(inputs, "path")?;
    let mut map = HashMap::new();
    map.insert("path".to_string(), serde_json::json!(path));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "fs.read_file",
        &format!("read:{path}"),
        false,
        &fs_read_manifest(),
        &map,
        || {
            let full = resolve_user_path(&path);
            let bytes = std::fs::read(&full)
                .map_err(|e| KernelError::Skill(format!("读 {} 失败: {e}", full.display())))?;
            let text = String::from_utf8_lossy(&bytes);
            if text.trim().is_empty() {
                return Ok(format!("{} 是空文件", full.display()));
            }
            Ok(format!(
                "{}：\n{}",
                full.display(),
                truncate_chars(&text, 8000)
            ))
        },
    )
}

pub fn fs_list_manifest() -> SkillManifest {
    simple_manifest(
        "fs.list_dir",
        "列目录",
        "列出目录下文件与子目录（目录优先，最多 200 项）。",
        &["列目录", "目录下有什么", "列出文件"],
        &["列一下这个目录", "目录里有什么文件"],
        vec![file_slot("path")],
        false,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_fs_list(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let path = slot_text(inputs, "path")?;
    let mut map = HashMap::new();
    map.insert("path".to_string(), serde_json::json!(path));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "fs.list_dir",
        &format!("list:{path}"),
        false,
        &fs_list_manifest(),
        &map,
        || {
            let full = resolve_user_path(&path);
            let rd = std::fs::read_dir(&full)
                .map_err(|e| KernelError::Skill(format!("列 {} 失败: {e}", full.display())))?;
            let mut dirs = vec![];
            let mut files = vec![];
            for entry in rd.filter_map(|e| e.ok()).take(201) {
                let name = entry.file_name().to_string_lossy().into_owned();
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    dirs.push(format!("{name}/"));
                } else {
                    files.push(name);
                }
            }
            dirs.sort();
            files.sort();
            let mut names = dirs;
            names.extend(files);
            if names.len() > 200 {
                names.truncate(200);
                names.push("…（已截断，只显示前 200 项）".to_string());
            }
            if names.is_empty() {
                return Ok(format!("{} 是空目录", full.display()));
            }
            Ok(format!(
                "{}（{} 项）：\n{}",
                full.display(),
                names.len(),
                names.join("\n")
            ))
        },
    )
}

pub fn fs_write_manifest() -> SkillManifest {
    simple_manifest(
        "fs.write_file",
        "写文件",
        "覆盖写入文本文件（不存在则创建父目录）。覆盖前旧内容备份为 .vp-bak。PerStep 审批。",
        &["写文件", "保存到文件", "存文件"],
        &["把这段文字写入文件", "保存到桌面文件"],
        vec![
            file_slot("path"),
            SimpleInput {
                name: "content",
                input_type: SkillInputType::Text,
                required: true,
                max_length: Some(200_000),
                allowed_roots: vec![],
            },
        ],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_fs_write(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let path = slot_text(inputs, "path")?;
    let content = slot_text(inputs, "content")?;
    let mut map = HashMap::new();
    map.insert("path".to_string(), serde_json::json!(path));
    map.insert("content".to_string(), serde_json::json!(content));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "fs.write_file",
        &format!("write:{path}"),
        true,
        &fs_write_manifest(),
        &map,
        || {
            let full = resolve_user_path(&path);
            if let Some(parent) = full.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        KernelError::Skill(format!("建父目录 {} 失败: {e}", parent.display()))
                    })?;
                }
            }
            let mut note = String::new();
            if full.is_file() {
                let bak = full.with_extension(format!(
                    "{}vp-bak",
                    full.extension()
                        .map(|e| format!("{}.", e.to_string_lossy()))
                        .unwrap_or_default()
                ));
                std::fs::copy(&full, &bak)
                    .map_err(|e| KernelError::Skill(format!("备份旧文件失败: {e}，拒绝覆盖")))?;
                note = format!("（旧内容已备份到 {}）", bak.display());
            }
            std::fs::write(&full, content.as_bytes())
                .map_err(|e| KernelError::Skill(format!("写 {} 失败: {e}", full.display())))?;
            Ok(format!(
                "已写入 {}（{} 字）{note}",
                full.display(),
                content.chars().count()
            ))
        },
    )
}

pub fn fs_create_manifest() -> SkillManifest {
    simple_manifest(
        "fs.create_file",
        "新建文件",
        "新建文本文件；已存在则报错（防误覆盖）。PerStep 审批。",
        &["新建文件", "创建文件", "建个文件"],
        &["新建一个文件", "创建一个空文件"],
        vec![
            file_slot("path"),
            SimpleInput {
                name: "content",
                input_type: SkillInputType::Text,
                required: false,
                max_length: Some(200_000),
                allowed_roots: vec![],
            },
        ],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_fs_create(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let path = slot_text(inputs, "path")?;
    let content = super::simple::slot_text_opt(inputs, "content").unwrap_or_default();
    let mut map = HashMap::new();
    map.insert("path".to_string(), serde_json::json!(path));
    if inputs.get("content").is_some() {
        map.insert("content".to_string(), serde_json::json!(content));
    }
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "fs.create_file",
        &format!("create:{path}"),
        true,
        &fs_create_manifest(),
        &map,
        || {
            let full = resolve_user_path(&path);
            if full.exists() {
                return Err(KernelError::Skill(format!(
                    "{} 已存在，拒绝覆盖（用写文件覆盖）",
                    full.display()
                )));
            }
            if let Some(parent) = full.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        KernelError::Skill(format!("建父目录 {} 失败: {e}", parent.display()))
                    })?;
                }
            }
            std::fs::write(&full, content.as_bytes())
                .map_err(|e| KernelError::Skill(format!("建 {} 失败: {e}", full.display())))?;
            Ok(format!("已新建 {}", full.display()))
        },
    )
}

pub fn fs_delete_manifest() -> SkillManifest {
    simple_manifest(
        "fs.delete_file",
        "删除文件",
        "可恢复删除：改名为 .vp-deleted-<时间戳>（同目录），非彻底抹除。PerStep 审批。",
        &["删除文件", "删文件", "移除文件"],
        &["删除这个文件", "把这个文件删掉"],
        vec![file_slot("path")],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

/// 可恢复删除目标名（纯函数，可测）。时间戳+pid 防同秒碰撞
///（Windows rename 撞名直接失败，fail-closed 但第二删必败）。
pub fn deleted_name(original: &std::path::Path, unix_ts: i64) -> PathBuf {
    let tag = format!("vp-deleted-{unix_ts}-{}", std::process::id());
    match (original.file_stem(), original.extension()) {
        (Some(stem), Some(ext)) => original.with_file_name(format!(
            "{}.{}.{}",
            stem.to_string_lossy(),
            tag,
            ext.to_string_lossy()
        )),
        _ => original.with_file_name(format!(
            "{}.{}",
            original
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "file".to_string()),
            tag
        )),
    }
}

pub fn execute_fs_delete(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let path = slot_text(inputs, "path")?;
    let mut map = HashMap::new();
    map.insert("path".to_string(), serde_json::json!(path));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "fs.delete_file",
        &format!("delete:{path}"),
        true,
        &fs_delete_manifest(),
        &map,
        || {
            let full = resolve_user_path(&path);
            if !full.is_file() {
                return Err(KernelError::Skill(format!(
                    "{} 不存在或不是文件（目录不删）",
                    full.display()
                )));
            }
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let grave = deleted_name(&full, ts);
            std::fs::rename(&full, &grave)
                .map_err(|e| KernelError::Skill(format!("删除 {} 失败: {e}", full.display())))?;
            Ok(format!(
                "已删除 {}（可恢复：改名为 {}，删掉该改名文件即彻底清除）",
                full.display(),
                grave.display()
            ))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::approver::{AutoApprover, AutoDenier};
    use crate::kernel::TrustKernel;

    fn tmp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vp-fsops-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn fs_manifests_have_ids() {
        for m in [
            fs_read_manifest(),
            fs_list_manifest(),
            fs_write_manifest(),
            fs_create_manifest(),
            fs_delete_manifest(),
        ] {
            assert!(!m.id.is_empty() && !m.keywords.is_empty(), "{}", m.id);
        }
    }

    #[test]
    fn resolve_user_path_passes_absolute_through() {
        let abs = if cfg!(windows) {
            r"C:\x\y.txt"
        } else {
            "/tmp/x.txt"
        };
        assert_eq!(resolve_user_path(abs), PathBuf::from(abs));
        // Documents 前缀进已知文件夹（机器相关，只断言后缀）。
        let p = resolve_user_path("Documents/a.txt");
        assert!(
            p.ends_with("Documents/a.txt") || p.ends_with("Documents\\a.txt"),
            "{p:?}"
        );
    }

    #[test]
    fn deleted_name_keeps_extension() {
        let grave = deleted_name(&PathBuf::from("/d/a.txt"), 123);
        let name = grave.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("a.vp-deleted-123-"), "{name}");
        assert!(name.ends_with(".txt"), "{name}");
    }

    #[test]
    fn write_then_read_then_recoverable_delete() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let f = tmp_file("roundtrip.txt");
        let _ = std::fs::remove_file(&f);
        let create = serde_json::json!({"path": f.to_string_lossy(), "content": "第一版"});
        execute_fs_create(&kernel, &AutoApprover, "t1", "s1", &create).unwrap();
        // 重复建 → 拒绝覆盖。
        assert!(execute_fs_create(&kernel, &AutoApprover, "t2", "s2", &create).is_err());
        // 覆盖写 → 留 .vp-bak。
        let write = serde_json::json!({"path": f.to_string_lossy(), "content": "第二版"});
        let out = execute_fs_write(&kernel, &AutoApprover, "t3", "s3", &write).unwrap();
        assert!(out.contains(".vp-bak"), "{out}");
        let read = serde_json::json!({"path": f.to_string_lossy()});
        let out = execute_fs_read(&kernel, &AutoApprover, "t4", "s4", &read).unwrap();
        assert!(out.contains("第二版"), "{out}");
        // 可恢复删除。
        let del = serde_json::json!({"path": f.to_string_lossy()});
        let out = execute_fs_delete(&kernel, &AutoApprover, "t5", "s5", &del).unwrap();
        assert!(out.contains("vp-deleted"), "{out}");
        assert!(!f.exists());
        // 清理残留。
        for entry in std::fs::read_dir(f.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
        {
            let n = entry.file_name().to_string_lossy().into_owned();
            if n.starts_with("roundtrip") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    #[test]
    fn write_denied_creates_nothing() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let f = tmp_file("denied.txt");
        let _ = std::fs::remove_file(&f);
        let inputs = serde_json::json!({"path": f.to_string_lossy(), "content": "x"});
        let err = execute_fs_write(&kernel, &AutoDenier, "t6", "s6", &inputs).unwrap_err();
        assert!(format!("{err:?}").contains("denied"), "{err:?}");
        assert!(!f.exists());
    }

    #[test]
    fn list_dir_shows_entries() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let dir = std::env::temp_dir().join(format!("vp-fsops-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 自建标记文件：不依赖其他测试的残留，并行安全。
        std::fs::write(dir.join("vp-list-marker.txt"), b"x").unwrap();
        let inputs = serde_json::json!({"path": dir.to_string_lossy()});
        let out = execute_fs_list(&kernel, &AutoApprover, "t7", "s7", &inputs).unwrap();
        assert!(out.contains("vp-list-marker.txt"), "{out}");
        let _ = std::fs::remove_file(dir.join("vp-list-marker.txt"));
    }
}
