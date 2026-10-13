-- T-303:步骤 upsert 需唯一约束(0022 的普通索引不足以支撑 ON CONFLICT)
DROP INDEX IF EXISTS idx_ci_step_run;
CREATE UNIQUE INDEX IF NOT EXISTS uq_ci_step_run
    ON ci_run_step (run_id, job_id, combo, seq);
