-- 工作项评论(T-132):详情页对话流
CREATE TABLE IF NOT EXISTS proj_work_item_comment (
    id           UUID PRIMARY KEY,
    work_item_id UUID        NOT NULL REFERENCES proj_work_item (id),
    author_id    UUID,
    body         TEXT        NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_proj_wic_comment_item
    ON proj_work_item_comment (work_item_id, created_at);
