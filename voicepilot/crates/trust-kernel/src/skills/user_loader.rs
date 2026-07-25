//! User-custom Skill loader — W7 Plan 3 §2.5.
//!
//! Scans `%APPDATA%\voicepilot\skills\*.md` (Windows), parses YAML
//! frontmatter + Markdown body, returns SkillManifests. Single-file
//! errors are logged and skipped; other files still load.

use std::path::{Path, PathBuf};

use regex::Regex;

use crate::error::{KernelError, Result};
use crate::skills::manifest::SkillManifest;

/// Max allowed file size: 1 MiB. Rejects oversize files before parsing
/// to prevent resource exhaustion (e.g., billion-laughs YAML).
const MAX_SKILL_FILE_BYTES: u64 = 1024 * 1024;

/// Skill id format: lowercase letter, then 0-63 of [a-z0-9._-].
/// Confines user skill ids to a safe namespace (no path separators,
/// no upper-case, no whitespace).
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

/// Parse a single Markdown + YAML frontmatter file into (SkillManifest, body).
///
/// Frontmatter format: file starts with `---\n`, ends at the next `---\n`.
/// Body is everything after the closing `---\n` (may be empty).
///
/// Validation:
///   - frontmatter present (opening + closing `---`)
///   - serde_yaml deserializes to SkillManifest
///   - skill.id matches `^[a-z][a-z0-9._-]{0,63}$`
pub fn parse_skill_md(content: &str) -> Result<(SkillManifest, String)> {
    // Split frontmatter from body. First line must be exactly `---`.
    // Find the closing `---` on its own line.
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() || lines[0].trim() != "---" {
        return Err(KernelError::Skill(
            "missing frontmatter opening ---".into(),
        ));
    }
    let mut close_idx = None;
    for (i, line) in lines.iter().enumerate().skip(1) {
        if line.trim() == "---" {
            close_idx = Some(i);
            break;
        }
    }
    let close_idx = close_idx.ok_or_else(|| {
        KernelError::Skill("missing frontmatter closing ---".into())
    })?;
    let frontmatter: String = lines[1..close_idx].join("\n");
    let body: String = lines[close_idx + 1..].join("\n");

    let mut manifest: SkillManifest = serde_yaml::from_str(&frontmatter)
        .map_err(|e| KernelError::Skill(format!("YAML parse failed: {}", e)))?;

    if !is_valid_skill_id(&manifest.id) {
        return Err(KernelError::Skill(format!(
            "invalid skill id: {} (must match ^[a-z][a-z0-9._-]{{0,63}}$)",
            manifest.id
        )));
    }
    manifest.description_body = if body.trim().is_empty() {
        None
    } else {
        Some(body.clone())
    };
    Ok((manifest, body))
}

/// Scan `dir` for `*.md` files and parse each. Returns successfully-parsed
/// SkillManifests. Parse failures are logged via tracing::warn! and skipped.
///
/// Non-recursive: only top-level files. Subdirectories are ignored.
pub fn scan_user_skills(dir: &Path) -> Vec<SkillManifest> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(error = ?e, dir = ?dir, "scan_user_skills: read_dir failed");
            return out;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Skip directories and non-.md files.
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        // Size check before reading.
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
                Ok((manifest, _)) => out.push(manifest),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_frontmatter(id: &str, title: &str) -> String {
        format!(
            "---\nid: {id}\nversion: \"1.0.0\"\ntitle: {title}\ndescription: desc\nintent_examples: []\nkeywords: []\ninputs: {{}}\nrisk_ceiling: E2\ndata_class_ceiling: D2\negress: local_only\nmax_steps: 3\ntools: []\napproval:\n  mode: none\n  required_for: commit\n  show_effect_manifest: false\n  max_approval_scope: 0\ncompensation:\n  level: none\n  ttl_seconds: 0\n  conflict_policy: auto_reverse\nverifier:\n  strategy: weak\n  recheck_after_seconds: 0\nfailure_policy:\n  max_retries: 0\n  allow_replan: false\n  on_fail: stop\n---\n\n# Body text\n\nThis is the description body.\n"
        )
    }

    #[test]
    fn parse_valid_md_returns_manifest_with_body() {
        let md = valid_frontmatter("my.test", "Test");
        let (m, body) = parse_skill_md(&md).expect("must parse");
        assert_eq!(m.id, "my.test");
        assert_eq!(m.title, "Test");
        assert!(body.contains("# Body text"));
        assert!(m.description_body.as_deref().unwrap().contains("# Body text"));
    }

    #[test]
    fn parse_missing_frontmatter_returns_error() {
        let md = "no frontmatter here";
        assert!(parse_skill_md(md).is_err());
    }

    #[test]
    fn parse_invalid_yaml_returns_error() {
        let md = "---\nid: my.test\nversion: \"1.0.0\"\ntitle: Test\nbad: [unclosed\n---\nbody\n";
        assert!(parse_skill_md(md).is_err());
    }

    #[test]
    fn parse_invalid_id_returns_error() {
        // Uppercase letter at start violates ^[a-z]...
        let md = "---\nid: My.Test\nversion: \"1.0.0\"\ntitle: Test\n---\nbody\n";
        assert!(parse_skill_md(md).is_err());
    }

    #[test]
    fn scan_user_skills_skips_malformed_and_returns_valid() {
        let dir = std::env::temp_dir().join(format!(
            "voicepilot-w7p3-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // valid.md
        std::fs::write(dir.join("valid.md"), valid_frontmatter("valid.skill", "V")).unwrap();
        // bad.md (invalid YAML)
        std::fs::write(dir.join("bad.md"), "---\nid: bad\nbad: [unclosed\n---\nbody\n").unwrap();
        // notmd.txt (ignored)
        std::fs::write(dir.join("notmd.txt"), "ignore me").unwrap();

        let manifests = scan_user_skills(&dir);
        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0].id, "valid.skill");

        std::fs::remove_dir_all(&dir).ok();
    }
}
