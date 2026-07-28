-- W8: DAG 编排相关表(V1.1.2 §5.4 LLM Planner + §6.2 prepare→approve→commit)
-- dag_plans: LLM 拆解生成的 DAG 计划
-- dag_nodes: DAG 节点执行状态
-- task_explanations: task.explain LLM 归因结果

CREATE TABLE IF NOT EXISTS dag_plans (
    plan_id TEXT PRIMARY KEY,
    user_goal TEXT NOT NULL,
    plan_json TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    completed_at TEXT,
    root_task_id TEXT,
    FOREIGN KEY (root_task_id) REFERENCES tasks(task_id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS dag_nodes (
    plan_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    skill_id TEXT NOT NULL,
    input_template_json TEXT NOT NULL,
    risk_ceiling TEXT NOT NULL,
    status TEXT NOT NULL,
    output_json TEXT,
    error_message TEXT,
    task_id TEXT,
    step_id TEXT,
    started_at TEXT,
    completed_at TEXT,
    PRIMARY KEY (plan_id, node_id),
    FOREIGN KEY (plan_id) REFERENCES dag_plans(plan_id) ON DELETE CASCADE,
    FOREIGN KEY (task_id) REFERENCES tasks(task_id) ON DELETE SET NULL,
    FOREIGN KEY (step_id) REFERENCES steps(step_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_dag_nodes_plan ON dag_nodes(plan_id);
CREATE INDEX IF NOT EXISTS idx_dag_plans_status ON dag_plans(status);

CREATE TABLE IF NOT EXISTS task_explanations (
    explanation_id TEXT PRIMARY KEY,
    step_id TEXT NOT NULL,
    root_cause_zh TEXT NOT NULL,
    category TEXT NOT NULL,
    suggested_fix TEXT,
    confidence REAL NOT NULL,
    llm_model TEXT,
    created_at TEXT NOT NULL,
    FOREIGN KEY (step_id) REFERENCES steps(step_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_task_explanations_step ON task_explanations(step_id);
