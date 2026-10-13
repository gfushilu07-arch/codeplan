-- 工时登记(T-318 / PM-09):工作项上的用时记录
CREATE TABLE IF NOT EXISTS proj_worklog (
    id           UUID PRIMARY KEY,
    work_item_id UUID        NOT NULL REFERENCES proj_work_item (id),
    author_id    UUID,
    minutes      INTEGER     NOT NULL,
    note         TEXT        NOT NULL DEFAULT '',
    spent_on     DATE        NOT NULL DEFAULT CURRENT_DATE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_proj_worklog_item
    ON proj_worklog (work_item_id, spent_on DESC);
