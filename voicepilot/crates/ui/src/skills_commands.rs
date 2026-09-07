//! Skills Manager Tauri commands —— V1.1.2 §8.3 Skills Manager 后端。

use std::path::{Path, PathBuf};
use tauri::State;

use crate::error::UiResult;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use trust_kernel::skills::manifest::SkillManifest;
use trust_kernel::skills::user_loader;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDto {
    pub skill_id: String,
    pub version: String,
    pub enabled: bool,
    pub success_count: i64,
    pub avg_latency_ms: f64,
    /// 从 manifest_json 解析出的风险等级(E×D),用于前端展示。
    pub risk_label: String,
}

impl From<trust_kernel::skills::repo::SkillRecord> for SkillDto {
    fn from(r: trust_kernel::skills::repo::SkillRecord) -> Self {
        let risk_label = serde_json::from_str::<serde_json::Value>(&r.manifest_json)
            .ok()
            .and_then(|v| v.get("risk").and_then(|r| r.as_str().map(String::from)))
            .unwrap_or_else(|| "unknown".to_string());
        Self {
            skill_id: r.skill_id,
            version: r.version.to_string(),
            enabled: r.enabled,
            success_count: r.success_count,
            avg_latency_ms: r.avg_latency_ms,
            risk_label,
        }
    }
}

/// W7 Plan 3: 用户自定义 Skill DTO(从 .md 文件加载,有别于 built-in)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSkillDto {
    pub skill_id: String,
    pub title: String,
    pub description: String,
    /// 是否绑定 MCP 工具可执行(metadata.voicepilot.execution 或 builtin)。
    pub executable: bool,
    /// 源文件绝对路径(`%APPDATA%\voicepilot\skills\<filename>.md`)。
    pub source_path: String,
    /// Phase A 密钥收编：导入时从伴随 .env 收编进 keyring 的凭据 key 名
    /// （仅名称；值已进 SecretStore，源文件未动）。空 = 无 .env 或无凭据。
    #[serde(default)]
    pub migrated_env_keys: Vec<String>,
}

impl UserSkillDto {
    fn from_manifest(m: &SkillManifest, source_path: PathBuf) -> Self {
        Self {
            skill_id: m.id.clone(),
            title: m.title.clone(),
            description: m.description.clone(),
            executable: m.execution.is_some(),
            source_path: source_path.to_string_lossy().into_owned(),
            migrated_env_keys: Vec::new(),
        }
    }
}

/// 逻辑函数:返回 UiResult,供测试直接调用。
pub fn list_skills(state: &AppState) -> UiResult<Vec<SkillDto>> {
    let records = state.kernel.list_skills()?;
    Ok(records.into_iter().map(SkillDto::from).collect())
}

/// 逻辑函数:返回 UiResult,供测试直接调用。
pub fn toggle_skill(state: &AppState, skill_id: &str, enabled: bool) -> UiResult<()> {
    state.kernel.toggle_skill(skill_id, enabled)?;
    Ok(())
}

/// W7 Plan 3 逻辑函数:重扫 skills 目录 + upsert 到 DB + 返回当前列表。
pub fn reload_skills(state: &AppState) -> UiResult<Vec<UserSkillDto>> {
    state.kernel.load_user_skills()?;
    list_user_skills(state)
}

/// W7 Plan 3 逻辑函数:列出当前用户自定义 Skill(重新扫描目录)。
pub fn list_user_skills(state: &AppState) -> UiResult<Vec<UserSkillDto>> {
    let dir = user_loader::user_skills_dir()
        .map_err(|e| crate::error::UiError::Tauri(e.to_string()))?;
    let manifests = state.kernel.list_user_skill_manifests()?;
    let mut dtos = Vec::new();
    for m in &manifests {
        // SkillManifest 不存原始文件名,按 id 反查目录下的 .md 文件。
        let path = find_skill_file_by_id(&dir, &m.id);
        dtos.push(UserSkillDto::from_manifest(m, path));
    }
    Ok(dtos)
}

/// W7 Plan 3 逻辑函数:校验 source_path + 复制到 skills 目录 + reload + 返回 DTO。
///
/// 安全:见 spec "Sink 2"。按以下顺序做边界校验:
///   1. 绝对路径
///   2. .md 扩展名
///   3. canonicalize(解析 symlink + `..`)
///   4. is_file
///   5. size ≤ 1 MiB
///
/// 复制目标 = `<skills_dir>/<file_name>`,file_name 仅取自 canonical.file_name(),
/// 不接受任何源路径组件。复制后再次 parse 验证,失败则删除并返回错误。
pub fn import_skill(state: &AppState, source_path: &str) -> UiResult<UserSkillDto> {
    // ===== Boundary validation =====
    let src = Path::new(source_path);
    if !src.is_absolute() {
        return Err(crate::error::UiError::InvalidConfig(
            "source_path must be absolute".into(),
        ));
    }
    if src.extension().and_then(|e| e.to_str()) != Some("md") {
        return Err(crate::error::UiError::InvalidConfig(
            "source_path must have .md extension".into(),
        ));
    }
    let canonical_src = std::fs::canonicalize(src)
        .map_err(|e| crate::error::UiError::Tauri(format!("canonicalize source failed: {}", e)))?;
    if !canonical_src.is_file() {
        return Err(crate::error::UiError::InvalidConfig(
            "source_path must be a regular file".into(),
        ));
    }
    let src_meta = std::fs::metadata(&canonical_src)
        .map_err(|e| crate::error::UiError::Tauri(format!("metadata failed: {}", e)))?;
    if src_meta.len() > 1024 * 1024 {
        return Err(crate::error::UiError::InvalidConfig(
            "skill file exceeds 1 MiB".into(),
        ));
    }

    // ===== 先解析源内容获取标准 id(决定目标目录名,防路径穿越) =====
    // parse 已校验 name(^[a-z][a-z0-9._-]{0,63}$),不含路径分隔符。
    let content = std::fs::read_to_string(&canonical_src).map_err(|e| {
        crate::error::UiError::Tauri(format!("read source failed: {}", e))
    })?;
    let (manifest, _) = user_loader::parse_skill_md(&content).map_err(|e| {
        crate::error::UiError::InvalidConfig(format!(
            "source is not a valid standard SKILL.md: {}",
            e
        ))
    })?;

    // ===== Destination path confinement(标准目录式:{id}/SKILL.md) =====
    let skills_dir = user_loader::user_skills_dir()
        .map_err(|e| crate::error::UiError::Tauri(e.to_string()))?;
    let dest_dir = skills_dir.join(&manifest.id);
    std::fs::create_dir_all(&dest_dir)
        .map_err(|e| crate::error::UiError::Tauri(format!("create skill dir failed: {}", e)))?;
    let dest = dest_dir.join("SKILL.md");
    // Defense-in-depth:目标必须在 skills_dir 内
    if !dest.starts_with(&skills_dir) {
        return Err(crate::error::UiError::InvalidConfig(
            "destination escapes skills dir".into(),
        ));
    }

    // ===== Copy (no shell) =====
    std::fs::copy(&canonical_src, &dest)
        .map_err(|e| crate::error::UiError::Tauri(format!("copy failed: {}", e)))?;

    // ===== Post-copy validation: re-parse to verify it's a valid standard SKILL.md =====
    let copied = std::fs::read_to_string(&dest)
        .map_err(|e| crate::error::UiError::Tauri(format!("read copied file failed: {}", e)))?;
    match user_loader::parse_skill_md(&copied) {
        Ok((m, _)) => {
            // Reload DB so the new skill is upserted.
            state.kernel.load_user_skills()?;
            Ok(UserSkillDto::from_manifest(&m, dest))
        }
        Err(e) => {
            // Clean up invalid file.
            let _ = std::fs::remove_file(&dest);
            let _ = std::fs::remove_dir(&dest_dir);
            Err(crate::error::UiError::InvalidConfig(format!(
                "copied file is not a valid standard SKILL.md: {}",
                e
            )))
        }
    }
}

/// Helper:扫描 `dir` 找到 .md 文件,其解析后的 manifest id == `id`。
/// 找不到时返回 fallback 路径(不应发生,调用方仅用于已加载的 manifest)。
fn find_skill_file_by_id(dir: &Path, id: &str) -> PathBuf {
    dir.join(id).join("SKILL.md")
}

/// 外部发现 Skill 候选项 DTO：只读扫描结果，不代表已安装/已启用。
/// 前端用已安装列表的 id 自行标注“已导入”。description + 执行绑定随附，
/// 避免用户盲导看不见的东西（description 驱动 keyword 路由，执行绑定决定
/// 启用后能调谁的工具）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalSkillDto {
    pub source: String,
    /// 跨源去重后的来源列表（同 id 同内容多来源合并为一个候选）。
    #[serde(default)]
    pub sources: Vec<String>,
    pub dir: String,
    pub id: String,
    pub title: String,
    pub description: String,
    pub executable: bool,
    pub exec_server: Option<String>,
    pub exec_tool: Option<String>,
}

fn to_external_skill_dto(
    h: trust_kernel::external_scan::ExternalSkillHit,
) -> ExternalSkillDto {
    ExternalSkillDto {
        source: h.source,
        sources: h.sources,
        dir: h.dir.to_string_lossy().into_owned(),
        id: h.id,
        title: h.title,
        description: h.description,
        executable: h.executable,
        exec_server: h.exec_server,
        exec_tool: h.exec_tool,
    }
}

/// 逻辑函数:扫描全局第三方 Skill 目录（Claude Code / Agent Skills 标准位置）。
/// 纯只读：不存在的根目录静默跳过，不写 DB、不碰网络、不起进程。
pub fn scan_external_skills(_state: &AppState) -> UiResult<Vec<ExternalSkillDto>> {
    Ok(scan_external_skills_with_roots(
        &trust_kernel::external_scan::default_windows_skill_roots(),
    ))
}

/// 可注入 roots 的扫描实现（单测用 TempDir 固件逐字段断言映射）。
pub fn scan_external_skills_with_roots(
    roots: &[(std::path::PathBuf, &'static str)],
) -> Vec<ExternalSkillDto> {
    use trust_kernel::external_scan::scan_external_skill_roots;
    scan_external_skill_roots(roots)
        .into_iter()
        .map(to_external_skill_dto)
        .collect()
}

/// 逻辑函数:导入外部 Skill 目录（只复制 SKILL.md，见 kernel 方法注释）。
/// 导入后默认关闭，用户在 Skills Manager 手动启用。目录伴随的 .env
/// 中疑似凭据会收编进 keyring（源文件不动），key 名随 DTO 返回。
pub fn import_external_skill(state: &AppState, dir: &str) -> UiResult<UserSkillDto> {
    let (manifest, migrated_env_keys) = state.kernel.import_external_skill_with_env(Path::new(dir))?;
    let path = user_loader::user_skills_dir()
        .map_err(|e| crate::error::UiError::Tauri(e.to_string()))?
        .join(&manifest.id)
        .join("SKILL.md");
    Ok(UserSkillDto {
        migrated_env_keys,
        ..UserSkillDto::from_manifest(&manifest, path)
    })
}

#[tauri::command]
pub async fn scan_external_skills_command(
    state: State<'_, AppState>,
) -> Result<Vec<ExternalSkillDto>, String> {
    scan_external_skills(&state).map_err(Into::into)
}

#[tauri::command]
pub async fn import_external_skill_command(
    state: State<'_, AppState>,
    dir: String,
) -> Result<UserSkillDto, String> {
    import_external_skill(&state, &dir).map_err(Into::into)
}

#[tauri::command]
pub async fn list_skills_command(
    state: State<'_, AppState>,
) -> Result<Vec<SkillDto>, String> {
    list_skills(&state).map_err(Into::into)
}

#[tauri::command]
pub async fn toggle_skill_command(
    state: State<'_, AppState>,
    skill_id: String,
    enabled: bool,
) -> Result<(), String> {
    toggle_skill(&state, &skill_id, enabled).map_err(Into::into)
}

/// W7 Plan 3: 重扫 skills 目录 + upsert 到 DB + 返回当前列表。
#[tauri::command]
pub async fn reload_skills_command(
    state: State<'_, AppState>,
) -> Result<Vec<UserSkillDto>, String> {
    reload_skills(&state).map_err(Into::into)
}

/// W7 Plan 3: 校验 + 复制 + reload。source_path 由前端通过
/// `@tauri-apps/plugin-dialog` 的 `open()` 选择(.md filter)。
#[tauri::command]
pub async fn import_skill_command(
    state: State<'_, AppState>,
    source_path: String,
) -> Result<UserSkillDto, String> {
    import_skill(&state, &source_path).map_err(Into::into)
}

/// W7 Plan 3: 列出当前用户自定义 Skill(重新扫描目录)。
#[tauri::command]
pub async fn list_user_skills_command(
    state: State<'_, AppState>,
) -> Result<Vec<UserSkillDto>, String> {
    list_user_skills(&state).map_err(Into::into)
}
