-- 看板排序与需求树(T-113)
ALTER TABLE proj_work_item ADD COLUMN IF NOT EXISTS board_order BIGINT NOT NULL DEFAULT 0;
ALTER TABLE proj_work_item ADD COLUMN IF NOT EXISTS parent_id UUID REFERENCES proj_work_item (id);
CREATE INDEX IF NOT EXISTS idx_proj_item_parent ON proj_work_item (parent_id);
