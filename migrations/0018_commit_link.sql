-- 提交关联(T-207):工作项 ↔ 提交链接(推送时解析 #KEY-N 自动建立)
CREATE TABLE IF NOT EXISTS proj_work_item_commit (
    id            UUID PRIMARY KEY,
    work_item_id  UUID         NOT NULL REFERENCES proj_work_item (id),
    repo_id       UUID         NOT NULL,
    full_name     VARCHAR(255) NOT NULL,
    commit_sha    VARCHAR(64)  NOT NULL,
    subject       TEXT         NOT NULL DEFAULT '',
    ref_name      VARCHAR(255) NOT NULL DEFAULT '',
    committed_at  TIMESTAMPTZ,
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT now(),
    UNIQUE (work_item_id, commit_sha, ref_name)
);
CREATE INDEX IF NOT EXISTS idx_proj_wic_item
    ON proj_work_item_commit (work_item_id, created_at DESC);
