//! prepare → approve → commit transaction protocol — V1.1 §6.2.
//!
//! Prevents TOCTOU (Time-of-Check-to-Time-of-Use) by:
//!   1. prepare: freeze a snapshot of affected files → effect_manifest +
//!      preconditions_hash + prepare_token (with expiry).
//!   2. approve: user reviews effect_manifest, grants approval_token.
//!   3. commit: re-check preconditions_hash; if mismatch → force re-prepare.
//!
//! W2 uses in-memory token store. W3 wires this to real filesystem MCP tools.

use crate::error::{KernelError, Result};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;

// Re-export FileSnapshot from tools::fs_snapshot so that policy::transaction
// and tools::fs_snapshot share a single type. W2 originally defined a local
// FileSnapshot here with identical fields, but W3a's FilesystemTool needs to
// push tools::fs_snapshot::FileSnapshot instances into EffectManifest.sources
// (Vec<FileSnapshot>). Without this re-export, the two structs are distinct
// types and the push would fail. See W3a Task 3 spec deviation note.
pub use crate::tools::fs_snapshot::FileSnapshot;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectManifest {
    pub sources: Vec<FileSnapshot>,
    pub destination: String,
    pub conflicts: Vec<String>,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrepareToken {
    pub token: String,
    pub expires_at: chrono::DateTime<Utc>,
    pub preconditions_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitResult {
    pub committed: bool,
    pub preconditions_recheck: String,
}

/// In-memory store of active prepare tokens. W3 will move to SQLite.
pub struct TransactionManager {
    tokens: Mutex<
        HashMap<
            String,
            (
                PrepareToken,
                String, /* task_id */
                String, /* step_id */
            ),
        >,
    >,
    ttl_seconds: i64,
}

impl Default for TransactionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TransactionManager {
    pub fn new() -> Self {
        Self {
            tokens: Mutex::new(HashMap::new()),
            ttl_seconds: 300, // 5 min prepare→commit window
        }
    }

    /// Compute the preconditions hash for a manifest.
    /// SHA256 over canonical JSON of all snapshot fields, sorted by path.
    pub fn preconditions_hash(&self, manifest: &EffectManifest) -> String {
        let mut sorted = manifest.sources.clone();
        sorted.sort_by(|a, b| a.canonical_path.cmp(&b.canonical_path));
        let mut hasher = Sha256::new();
        for snap in &sorted {
            hasher.update(snap.canonical_path.as_bytes());
            hasher.update(snap.file_id.as_bytes());
            hasher.update(snap.size.to_le_bytes());
            hasher.update(snap.last_write_time.as_bytes());
            hasher.update(snap.sha256.as_bytes());
        }
        hasher.update(manifest.destination.as_bytes());
        hasher.update(manifest.total_bytes.to_le_bytes());
        format!("sha256:{:x}", hasher.finalize())
    }

    /// Issue a prepare token for the given manifest.
    pub fn prepare(
        &self,
        task_id: &str,
        step_id: &str,
        manifest: &EffectManifest,
    ) -> Result<PrepareToken> {
        self.prepare_with_ttl(task_id, step_id, manifest, self.ttl_seconds)
    }

    /// 以指定 TTL（秒）签发 prepare token。
    ///
    /// 生产路径走 `prepare`（固定 300s）；本方法供测试构造「已过期」token，
    /// 从而在 HTTP 层真实验证 `validate` 的过期分支（不需要真的等 5 分钟）。
    pub fn prepare_with_ttl(
        &self,
        task_id: &str,
        step_id: &str,
        manifest: &EffectManifest,
        ttl_seconds: i64,
    ) -> Result<PrepareToken> {
        let token = PrepareToken {
            token: format!("prt_{}", uuid::Uuid::new_v4()),
            expires_at: Utc::now() + Duration::seconds(ttl_seconds),
            preconditions_hash: self.preconditions_hash(manifest),
        };
        self.tokens.lock().unwrap().insert(
            token.token.clone(),
            (token.clone(), task_id.to_string(), step_id.to_string()),
        );
        Ok(token)
    }

    /// 只读预检 prepare token（契约 §3.3，HTTP 网关执行前的 TOCTOU 校验）。
    ///
    /// 三步校验，顺序与 `commit` 一致：① token 不存在 → `InvalidPrepareToken`；
    /// ② `stored.expires_at < now`（TTL=300s）→ `InvalidPrepareToken`；
    /// ③ `preconditions_hash(manifest) != stored.preconditions_hash` →
    /// `PreconditionMismatch`。
    ///
    /// 与 `commit` 的区别：**不消费 token** —— 预检可以重复调用，`commit` 才是一次性
    /// 消费（否则一次预检就会把 token 烧掉，重试必失败）。
    pub fn validate(&self, token: &str, manifest: &EffectManifest) -> Result<()> {
        // 短锁：只克隆条目，出 block 后锁已释放（preconditions_hash 不需要锁）。
        let entry = {
            let tokens = self.tokens.lock().unwrap();
            tokens.get(token).cloned()
        };
        let (stored, _task_id, _step_id) =
            entry.ok_or_else(|| KernelError::InvalidPrepareToken(token.to_string()))?;

        if Utc::now() > stored.expires_at {
            return Err(KernelError::InvalidPrepareToken(format!(
                "expired: {token}"
            )));
        }

        let actual_hash = self.preconditions_hash(manifest);
        if actual_hash != stored.preconditions_hash {
            return Err(KernelError::PreconditionMismatch {
                expected: stored.preconditions_hash,
                actual: actual_hash,
            });
        }

        Ok(())
    }

    /// Attempt to commit. Re-checks preconditions_hash; rejects mismatch or expiry.
    pub fn commit(&self, token: &PrepareToken, manifest: &EffectManifest) -> Result<CommitResult> {
        let entry = {
            let tokens = self.tokens.lock().unwrap();
            tokens.get(&token.token).cloned()
        };
        let (stored, _task_id, _step_id) =
            entry.ok_or_else(|| KernelError::InvalidPrepareToken(token.token.clone()))?;

        // Check both the caller-supplied and server-stored expiries.
        // - token.expires_at: caller's view (test simulates time passage by mutating it).
        // - stored.expires_at: authoritative server-side expiry (cannot be bypassed by caller).
        let now = Utc::now();
        if now > token.expires_at || now > stored.expires_at {
            return Err(KernelError::InvalidPrepareToken(format!(
                "expired: {}",
                token.token
            )));
        }

        let actual_hash = self.preconditions_hash(manifest);
        if actual_hash != stored.preconditions_hash {
            return Err(KernelError::PreconditionMismatch {
                expected: stored.preconditions_hash,
                actual: actual_hash,
            });
        }

        // Consume the token (one-shot commit).
        self.tokens.lock().unwrap().remove(&token.token);

        Ok(CommitResult {
            committed: true,
            preconditions_recheck: actual_hash,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(path: &str, size: u64) -> FileSnapshot {
        FileSnapshot {
            canonical_path: path.to_string(),
            file_id: format!("id-{path}"),
            size,
            last_write_time: "2026-01-01T00:00:00Z".to_string(),
            sha256: format!("sha256:{path}"),
        }
    }

    fn manifest(size: u64) -> EffectManifest {
        EffectManifest {
            sources: vec![snap("C:/a.txt", size)],
            destination: "C:/dest".to_string(),
            conflicts: vec![],
            total_bytes: size,
        }
    }

    #[test]
    fn validate_accepts_matching_manifest_and_keeps_token() {
        let mgr = TransactionManager::new();
        let m = manifest(10);
        let token = mgr.prepare("t1", "s1", &m).unwrap();

        mgr.validate(&token.token, &m).expect("validate must pass");
        // 只读预检：可以重复调用，且不消费 token。
        mgr.validate(&token.token, &m)
            .expect("validate is idempotent");
        mgr.commit(&token, &m)
            .expect("token must survive validate (commit consumes it)");
    }

    #[test]
    fn validate_rejects_unknown_token() {
        let mgr = TransactionManager::new();
        let err = mgr
            .validate("prt_does_not_exist", &manifest(1))
            .expect_err("unknown token must fail");
        assert!(matches!(err, KernelError::InvalidPrepareToken(_)));
    }

    #[test]
    fn validate_rejects_expired_token() {
        let mgr = TransactionManager::new();
        let m = manifest(2);
        let token = mgr.prepare("t2", "s2", &m).unwrap();
        // 直接改服务端存储的过期时间（同模块可访问私有字段），模拟时间流逝。
        {
            let mut tokens = mgr.tokens.lock().unwrap();
            let entry = tokens.get_mut(&token.token).unwrap();
            entry.0.expires_at = Utc::now() - Duration::seconds(1);
        }
        let err = mgr
            .validate(&token.token, &m)
            .expect_err("expired token must fail");
        assert!(matches!(err, KernelError::InvalidPrepareToken(_)));
    }

    #[test]
    fn validate_rejects_tampered_manifest() {
        let mgr = TransactionManager::new();
        let prepared = manifest(3);
        let token = mgr.prepare("t3", "s3", &prepared).unwrap();
        let tampered = manifest(4); // total_bytes / size 被改
        let err = mgr
            .validate(&token.token, &tampered)
            .expect_err("tampered manifest must fail");
        assert!(matches!(err, KernelError::PreconditionMismatch { .. }));
    }
}
