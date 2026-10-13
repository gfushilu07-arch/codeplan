-- DORA 事实表(T-307):部署 / 变更 / 失败 三类事件的聚合事实
CREATE TABLE IF NOT EXISTS ins_dora_fact (
    id                UUID PRIMARY KEY,
    repo_id           UUID         NOT NULL,
    kind              VARCHAR(24)  NOT NULL,
    occurred_at       TIMESTAMPTZ  NOT NULL,
    lead_time_seconds BIGINT,
    subject           VARCHAR(255) NOT NULL DEFAULT '',
    ref               VARCHAR(255) NOT NULL DEFAULT '',
    detail            JSONB        NOT NULL DEFAULT '{}'::jsonb,
    created_at        TIMESTAMPTZ  NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_ins_dora_fact_repo_time
    ON ins_dora_fact (repo_id, occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_ins_dora_fact_kind
    ON ins_dora_fact (repo_id, kind, occurred_at DESC);
-- 幂等:同一仓库同一引用同一类事件只记一次(重放/重试安全)
CREATE UNIQUE INDEX IF NOT EXISTS uq_ins_dora_fact_event
    ON ins_dora_fact (repo_id, kind, ref, occurred_at);
