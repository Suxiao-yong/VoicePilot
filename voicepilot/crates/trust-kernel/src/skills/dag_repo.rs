//! DagRepo — W8 §2.6.
//!
//! CRUD for `dag_plans` + `dag_nodes` tables.
//! 遵循 W4 McpServerRepo 模式:`new()` 不带参数,方法接收 `&Connection`。

use chrono::Utc;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::skills::dag_types::{DagNode, DagNodeStatus, DagPlan, DagStatus};

/// DB 持久化形态(序列化字段用 JSON)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagPlanRecord {
    pub plan_id: String,
    pub user_goal: String,
    pub plan_json: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub root_task_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagNodeRecord {
    pub plan_id: String,
    pub node_id: String,
    pub skill_id: String,
    pub input_template_json: String,
    pub risk_ceiling: String,
    pub status: String,
    pub output_json: Option<String>,
    pub error_message: Option<String>,
    pub task_id: Option<String>,
    pub step_id: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

pub struct DagRepo;

impl DagRepo {
    pub fn new() -> Self {
        Self
    }

    /// 创建 DAG plan 记录。
    pub fn create_plan(
        &self,
        conn: &Connection,
        plan: &DagPlan,
        status: &DagStatus,
        root_task_id: Option<&str>,
    ) -> Result<()> {
        let plan_json = serde_json::to_string(plan).map_err(|e| {
            crate::error::KernelError::Db(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
        })?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            r#"INSERT INTO dag_plans (plan_id, user_goal, plan_json, status, created_at, completed_at, root_task_id)
               VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)"#,
            params![
                plan.plan_id,
                plan.user_goal,
                plan_json,
                status.as_str(),
                now,
                root_task_id,
            ],
        )?;
        Ok(())
    }

    pub fn get_plan(&self, conn: &Connection, plan_id: &str) -> Result<Option<DagPlanRecord>> {
        let mut stmt = conn.prepare(
            r#"SELECT plan_id, user_goal, plan_json, status, created_at, completed_at, root_task_id
               FROM dag_plans WHERE plan_id = ?1"#,
        )?;
        let mut rows = stmt.query(params![plan_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(DagPlanRecord {
                plan_id: row.get(0)?,
                user_goal: row.get(1)?,
                plan_json: row.get(2)?,
                status: row.get(3)?,
                created_at: row.get(4)?,
                completed_at: row.get(5)?,
                root_task_id: row.get(6)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// 按 status 列表查询 plan(W8 UI 显示"运行中" / "已完成" DAG)。
    pub fn list_plans_by_status(
        &self,
        conn: &Connection,
        status: &str,
    ) -> Result<Vec<DagPlanRecord>> {
        let mut stmt = conn.prepare(
            r#"SELECT plan_id, user_goal, plan_json, status, created_at, completed_at, root_task_id
               FROM dag_plans WHERE status = ?1 ORDER BY created_at DESC"#,
        )?;
        let rows = stmt.query_map(params![status], |row| {
            Ok(DagPlanRecord {
                plan_id: row.get(0)?,
                user_goal: row.get(1)?,
                plan_json: row.get(2)?,
                status: row.get(3)?,
                created_at: row.get(4)?,
                completed_at: row.get(5)?,
                root_task_id: row.get(6)?,
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 更新 DAG plan 状态(DagExecutor 执行后调用)。
    /// `completed = true` 时写入 `completed_at` 时间戳。
    pub fn update_plan_status(
        &self,
        conn: &Connection,
        plan_id: &str,
        status: &DagStatus,
        completed: bool,
    ) -> Result<()> {
        let now = if completed {
            Some(Utc::now().to_rfc3339())
        } else {
            None
        };
        conn.execute(
            r#"UPDATE dag_plans
               SET status = ?1, completed_at = COALESCE(?2, completed_at)
               WHERE plan_id = ?3"#,
            params![status.as_str(), now, plan_id],
        )?;
        Ok(())
    }

    /// 创建 DAG 节点记录(初始状态 = pending,started_at = now)。
    pub fn create_node(&self, conn: &Connection, plan_id: &str, node: &DagNode) -> Result<()> {
        let input_template_json = serde_json::to_string(&node.input_template).map_err(|e| {
            crate::error::KernelError::Db(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
        })?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            r#"INSERT INTO dag_nodes
               (plan_id, node_id, skill_id, input_template_json, risk_ceiling,
                status, output_json, error_message, task_id, step_id,
                started_at, completed_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, NULL, NULL, ?7, NULL)"#,
            params![
                plan_id,
                node.node_id,
                node.skill_id,
                input_template_json,
                node.risk_ceiling.as_str(),
                DagNodeStatus::Pending.as_str(),
                now,
            ],
        )?;
        Ok(())
    }

    /// 更新 DAG 节点状态(DagExecutor 节点执行后调用)。
    /// `Succeeded` / `Failed` / `Skipped` 视为终态,自动写入 `completed_at`。
    pub fn update_node_status(
        &self,
        conn: &Connection,
        plan_id: &str,
        node_id: &str,
        status: &DagNodeStatus,
        task_id: Option<&str>,
        step_id: Option<&str>,
    ) -> Result<()> {
        let (output_json, error_msg) = match status {
            DagNodeStatus::Succeeded(out) => {
                (Some(serde_json::to_string(out).unwrap_or_default()), None)
            }
            DagNodeStatus::Failed { cause } => (None, Some(cause.clone())),
            _ => (None, None),
        };
        let completed = matches!(
            status,
            DagNodeStatus::Succeeded(_) | DagNodeStatus::Failed { .. } | DagNodeStatus::Skipped
        );
        let now = if completed {
            Some(Utc::now().to_rfc3339())
        } else {
            None
        };
        conn.execute(
            r#"UPDATE dag_nodes
               SET status = ?1,
                   output_json = ?2,
                   error_message = ?3,
                   task_id = COALESCE(?4, task_id),
                   step_id = COALESCE(?5, step_id),
                   completed_at = COALESCE(?6, completed_at)
               WHERE plan_id = ?7 AND node_id = ?8"#,
            params![
                status.as_str(),
                output_json,
                error_msg,
                task_id,
                step_id,
                now,
                plan_id,
                node_id,
            ],
        )?;
        Ok(())
    }

    /// 列出某 DAG plan 的所有节点(按 started_at 排序)。
    pub fn list_nodes_by_plan(
        &self,
        conn: &Connection,
        plan_id: &str,
    ) -> Result<Vec<DagNodeRecord>> {
        let mut stmt = conn.prepare(
            r#"SELECT plan_id, node_id, skill_id, input_template_json, risk_ceiling,
                      status, output_json, error_message, task_id, step_id,
                      started_at, completed_at
               FROM dag_nodes WHERE plan_id = ?1 ORDER BY started_at ASC"#,
        )?;
        let rows = stmt.query_map(params![plan_id], |row| {
            Ok(DagNodeRecord {
                plan_id: row.get(0)?,
                node_id: row.get(1)?,
                skill_id: row.get(2)?,
                input_template_json: row.get(3)?,
                risk_ceiling: row.get(4)?,
                status: row.get(5)?,
                output_json: row.get(6)?,
                error_message: row.get(7)?,
                task_id: row.get(8)?,
                step_id: row.get(9)?,
                started_at: row.get(10)?,
                completed_at: row.get(11)?,
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 级联删除 DAG plan + 其所有节点。
    /// (FK ON DELETE CASCADE 会自动删 dag_nodes,但显式删便于审计)
    pub fn delete_plan_cascade(&self, conn: &Connection, plan_id: &str) -> Result<()> {
        conn.execute(
            r#"DELETE FROM dag_nodes WHERE plan_id = ?1"#,
            params![plan_id],
        )?;
        conn.execute(
            r#"DELETE FROM dag_plans WHERE plan_id = ?1"#,
            params![plan_id],
        )?;
        Ok(())
    }
}

impl Default for DagRepo {
    fn default() -> Self {
        Self::new()
    }
}
