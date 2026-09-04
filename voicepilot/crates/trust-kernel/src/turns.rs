//! turns 情景记忆：每轮封轮一条摘要（结果可回忆，原始音频永不落盘）。

use crate::error::Result;
use rusqlite::{params, Connection};

/// outcome 取值：`routed:<skill_id>` | `dag` | `unmatched` | `empty` | `nospeech` | `error`。
/// transcript 截断到 500 字符后存入；privacy_mode 下调用方不得调用本模块。
#[derive(Debug, Clone)]
pub struct TurnRecord {
    pub turn_id: String,
    pub started_at_ms: i64,
    pub source: String,
    pub transcript: String,
    pub outcome: String,
    pub plan_id: String,
    pub latency_ms: i64,
    pub sensitive: bool,
}

pub fn record_turn(conn: &Connection, rec: &TurnRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO turns (turn_id, started_at_ms, source, transcript, outcome, plan_id, latency_ms, sensitive)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(turn_id) DO UPDATE SET
           started_at_ms = excluded.started_at_ms, source = excluded.source,
           transcript = excluded.transcript, outcome = excluded.outcome,
           plan_id = excluded.plan_id, latency_ms = excluded.latency_ms,
           sensitive = excluded.sensitive",
        params![
            rec.turn_id,
            rec.started_at_ms,
            rec.source,
            rec.transcript,
            rec.outcome,
            rec.plan_id,
            rec.latency_ms,
            rec.sensitive as i32
        ],
    )?;
    Ok(())
}

/// 取最近 N 轮（时间倒序）。回忆时调用方自行反转为正序渲染。
pub fn recent_turns(conn: &Connection, limit: usize) -> Result<Vec<TurnRecord>> {
    let mut stmt = conn.prepare(
        "SELECT turn_id, started_at_ms, source, transcript, outcome, plan_id, latency_ms, sensitive
         FROM turns ORDER BY started_at_ms DESC, rowid DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], |row| {
        Ok(TurnRecord {
            turn_id: row.get(0)?,
            started_at_ms: row.get(1)?,
            source: row.get(2)?,
            transcript: row.get(3)?,
            outcome: row.get(4)?,
            plan_id: row.get(5)?,
            latency_ms: row.get(6)?,
            sensitive: row.get::<_, i32>(7)? != 0,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
}

/// TTL 清理：删除早于 now_ms - days*86400*1000 的轮次，返回删除行数。
pub fn prune_turns_older_than(conn: &Connection, days: u32, now_ms: i64) -> Result<usize> {
    let cutoff = now_ms - days as i64 * 86_400_000;
    let deleted = conn.execute("DELETE FROM turns WHERE started_at_ms < ?1", params![cutoff])?;
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::run_migrations;

    fn migrated() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn
    }

    fn sample(id: &str, started_at_ms: i64) -> TurnRecord {
        TurnRecord {
            turn_id: id.to_string(),
            started_at_ms,
            source: "voice".to_string(),
            transcript: "打开记事本".to_string(),
            outcome: "routed:quick.app_control".to_string(),
            plan_id: String::new(),
            latency_ms: 320,
            sensitive: false,
        }
    }

    #[test]
    fn record_and_recent_return_newest_first() {
        let conn = migrated();
        record_turn(&conn, &sample("t1", 1000)).unwrap();
        record_turn(&conn, &sample("t2", 2000)).unwrap();
        let rows = recent_turns(&conn, 3).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].turn_id, "t2");
        assert_eq!(rows[1].turn_id, "t1");
    }

    #[test]
    fn prune_removes_only_expired_turns() {
        let conn = migrated();
        record_turn(&conn, &sample("old", 1000)).unwrap();
        record_turn(&conn, &sample("new", 1000 + 31 * 86_400_000)).unwrap();
        let deleted = prune_turns_older_than(&conn, 30, 1000 + 31 * 86_400_000).unwrap();
        assert_eq!(deleted, 1);
        let rows = recent_turns(&conn, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].turn_id, "new");
    }
}
