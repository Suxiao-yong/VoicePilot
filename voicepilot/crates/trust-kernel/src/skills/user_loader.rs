//! User-custom Skill loader — Agent Skills 开放标准适配（2026-08-24 统一）。
//!
//! Scans `%APPDATA%\voicepilot\skills\{name}\SKILL.md` (Windows), parses the
//! standard six-field frontmatter (agentskills.io) + Markdown body, returns
//! SkillManifests with default security posture. Per-skill errors are logged
//! and skipped; other skills still load. Legacy single-file `*.md` skills are
//! no longer loaded (完全移除单文件).

use std::path::{Path, PathBuf};

use regex::Regex;

use crate::error::{KernelError, Result};
use crate::skills::manifest::{SkillManifest, StandardSkillSpec};

/// Max allowed file size: 1 MiB. Rejects oversize files before parsing
/// to prevent resource exhaustion (e.g., billion-laughs YAML).
const MAX_SKILL_FILE_BYTES: u64 = 1024 * 1024;

/// Skill id format: lowercase letter, then 0-63 of [a-z0-9._-].
/// Confines user skill ids to a safe namespace (no path separators,
/// no upper-case, no whitespace). Same constraint as the standard `name`.
fn is_valid_skill_id(id: &str) -> bool {
    let re = Regex::new(r"^[a-z][a-z0-9._-]{0,63}$").unwrap();
    re.is_match(id)
}

/// Compute the user skills directory: `%APPDATA%\voicepilot\skills` on Windows.
/// Creates the directory if missing. Returns the canonicalized path.
pub fn user_skills_dir() -> Result<PathBuf> {
    let base = dirs::data_dir()
        .ok_or_else(|| KernelError::Skill("dirs::data_dir() returned None".into()))?;
    let dir = base.join("voicepilot").join("skills");
    std::fs::create_dir_all(&dir)?;
    // canonicalize to resolve any symlinks in %APPDATA% path
    std::fs::canonicalize(&dir).map_err(KernelError::Io)
}

/// Parse a standard SKILL.md into (SkillManifest, body).
///
/// Frontmatter format: file starts with `---\n`, ends at the next `---\n`.
/// Body is everything after the closing `---\n` (may be empty).
///
/// Validation:
///   - frontmatter present (opening + closing `---`)
///   - serde_yaml deserializes to the standard six fields + optional metadata
///   - `name` matches `^[a-z][a-z0-9._-]{0,63}$` (used as `id`)
///   - `description` non-empty, ≤ 1024 chars
pub fn parse_skill_md(content: &str) -> Result<(SkillManifest, String)> {
    // Split frontmatter from body. First line must be exactly `---`.
    // Find the closing `---` on its own line.
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() || lines[0].trim() != "---" {
        return Err(KernelError::Skill("missing frontmatter opening ---".into()));
    }
    let mut close_idx = None;
    for (i, line) in lines.iter().enumerate().skip(1) {
        if line.trim() == "---" {
            close_idx = Some(i);
            break;
        }
    }
    let close_idx =
        close_idx.ok_or_else(|| KernelError::Skill("missing frontmatter closing ---".into()))?;
    let frontmatter: String = lines[1..close_idx].join("\n");
    let body: String = lines[close_idx + 1..].join("\n");

    let spec: StandardSkillSpec = serde_yaml::from_str(&frontmatter)
        .map_err(|e| KernelError::Skill(format!("YAML parse failed: {}", e)))?;

    if !is_valid_skill_id(&spec.name) {
        return Err(KernelError::Skill(format!(
            "invalid skill name: {} (must match ^[a-z][a-z0-9._-]{{0,63}}$)",
            spec.name
        )));
    }
    if spec.description.trim().is_empty() {
        return Err(KernelError::Skill(
            "description is required (standard frontmatter field)".into(),
        ));
    }
    if spec.description.chars().count() > 1024 {
        return Err(KernelError::Skill(
            "description too long (max 1024 chars)".into(),
        ));
    }

    let manifest = spec.to_manifest(body.clone());
    Ok((manifest, body))
}

#[derive(Debug, Clone)]
pub struct UserSkillFile {
    pub path: PathBuf,
    pub manifest: SkillManifest,
}

/// Scan `dir` for standard skill directories (`{name}/SKILL.md`) and retain
/// each parsed manifest's actual path. Parse failures are logged and skipped.
/// Non-recursive; legacy `.md` single files are ignored.
pub fn scan_user_skill_files(dir: &Path) -> Vec<UserSkillFile> {
    let mut skill_mds: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .map(|d| d.join("SKILL.md"))
            .filter(|p| p.is_file())
            .collect(),
        Err(e) => {
            tracing::warn!(error = ?e, dir = ?dir, "scan_user_skill_files: read_dir failed");
            return Vec::new();
        }
    };
    skill_mds.sort();

    let mut out = Vec::new();
    for path in skill_mds {
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.len() > MAX_SKILL_FILE_BYTES {
                tracing::warn!(
                    skill_load_error = ?path,
                    size = meta.len(),
                    max = MAX_SKILL_FILE_BYTES,
                    "skipping oversize skill file"
                );
                continue;
            }
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => match parse_skill_md(&content) {
                Ok((manifest, _)) => out.push(UserSkillFile { path, manifest }),
                Err(e) => tracing::warn!(
                    skill_load_error = ?path,
                    error = ?e,
                    "skipping malformed skill file"
                ),
            },
            Err(e) => tracing::warn!(
                skill_load_error = ?path,
                error = ?e,
                "failed to read skill file"
            ),
        }
    }
    out
}

/// Compatibility wrapper for callers that only need manifests.
pub fn scan_user_skills(dir: &Path) -> Vec<SkillManifest> {
    scan_user_skill_files(dir)
        .into_iter()
        .map(|file| file.manifest)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::types::ELevel;
    use crate::skills::manifest::SkillExecutionSpec;

    /// Standard six-field frontmatter + optional voicepilot execution binding.
    fn standard_frontmatter(name: &str, with_execution: bool) -> String {
        let exec = if with_execution {
            "metadata:\n  voicepilot:\n    execution:\n      type: mcp_tool\n      server_id: my-server\n      tool_name: my_tool\n"
        } else {
            ""
        };
        format!(
            "---\nname: {name}\ndescription: Organize downloaded files by moving PDFs into folders.\n{exec}---\n\n# Body text\n\nThis is the description body.\n"
        )
    }

    #[test]
    fn parse_valid_standard_md_returns_manifest_with_body() {
        let md = standard_frontmatter("file-organizer", false);
        let (m, body) = parse_skill_md(&md).expect("must parse");
        // id 来自标准 name
        assert_eq!(m.id, "file-organizer");
        assert_eq!(m.title, "file-organizer");
        assert!(body.contains("# Body text"));
        assert!(
            m.description_body
                .as_deref()
                .unwrap()
                .contains("# Body text")
        );
        // 默认安全兜底
        assert_eq!(m.risk_ceiling, ELevel::E1);
        assert_eq!(m.max_steps, 8);
        assert!(m.execution.is_none());
        // 关键词从 description 派生(英文 token)
        assert!(m.keywords.iter().any(|k| k == "organize"));
    }

    #[test]
    fn parse_metadata_voicepilot_execution_binds_mcp_tool() {
        let md = standard_frontmatter("bind-me", true);
        let (m, _) = parse_skill_md(&md).expect("must parse");
        match m.execution {
            Some(SkillExecutionSpec::McpTool {
                server_id,
                tool_name,
            }) => {
                assert_eq!(server_id, "my-server");
                assert_eq!(tool_name, "my_tool");
            }
            other => panic!("expected McpTool binding, got {other:?}"),
        }
        // 工具授权至少包含绑定工具
        assert!(m.tools.iter().any(|t| t == "my_tool"));
    }

    #[test]
    fn parse_missing_frontmatter_returns_error() {
        let md = "no frontmatter here";
        assert!(parse_skill_md(md).is_err());
    }

    #[test]
    fn parse_invalid_yaml_returns_error() {
        let md = "---\nname: my-test\nbad: [unclosed\n---\nbody\n";
        assert!(parse_skill_md(md).is_err());
    }

    #[test]
    fn parse_invalid_name_returns_error() {
        // Uppercase letter at start violates ^[a-z]...
        let md = "---\nname: My-Test\ndescription: desc\n---\nbody\n";
        assert!(parse_skill_md(md).is_err());
    }

    #[test]
    fn parse_missing_description_returns_error() {
        let md = "---\nname: my-test\n---\nbody\n";
        assert!(parse_skill_md(md).is_err());
    }

    #[test]
    fn scan_user_skills_loads_directories_and_ignores_legacy_md() {
        let dir = std::env::temp_dir().join(format!(
            "voicepilot-std-skill-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // 标准目录式
        let ok_dir = dir.join("valid-skill");
        std::fs::create_dir_all(&ok_dir).unwrap();
        std::fs::write(
            ok_dir.join("SKILL.md"),
            standard_frontmatter("valid-skill", false),
        )
        .unwrap();
        // 坏目录(非法 name)
        let bad_dir = dir.join("Bad-Name");
        std::fs::create_dir_all(&bad_dir).unwrap();
        std::fs::write(
            bad_dir.join("SKILL.md"),
            "---\nname: Bad-Name\ndescription: d\n---\nbody\n",
        )
        .unwrap();
        // 旧单文件(必须被忽略)
        std::fs::write(dir.join("legacy.md"), standard_frontmatter("legacy", false)).unwrap();
        // 非 skill 文件
        std::fs::write(dir.join("notmd.txt"), "ignore me").unwrap();

        let manifests = scan_user_skills(&dir);
        assert_eq!(manifests.len(), 1, "只加载标准目录式 SKILL.md");
        assert_eq!(manifests[0].id, "valid-skill");

        std::fs::remove_dir_all(&dir).ok();
    }
}
