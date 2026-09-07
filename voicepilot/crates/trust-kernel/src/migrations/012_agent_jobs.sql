-- agent_jobs 表：进程内后台作业（OpenClaw Automations 复现，计划 Phase C）。
-- 已知边界（写死在 scheduler.rs 文档）：进程退出期间错过的不追补；
-- 重启后从 next_run_at_ms 续跑。
-- agent_job_runs：作业执行历史（job_id 外键可溯，R1 finding 2）。

CREATE TABLE IF NOT EXISTS agent_jobs (
    job_id TEXT PRIMARY KEY,
    prompt TEXT NOT NULL,
    schedule TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at_ms INTEGER NOT NULL,
    last_run_at_ms INTEGER,
    next_run_at_ms INTEGER
);

CREATE TABLE IF NOT EXISTS agent_job_runs (
    run_id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL REFERENCES agent_jobs (job_id) ON DELETE CASCADE,
    started_at_ms INTEGER NOT NULL,
    finished_at_ms INTEGER NOT NULL,
    outcome TEXT NOT NULL DEFAULT 'failed',
    result TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_agent_jobs_due ON agent_jobs (
    enabled, next_run_at_ms
);
CREATE INDEX IF NOT EXISTS idx_agent_job_runs_job ON agent_job_runs (
    job_id, started_at_ms
);
