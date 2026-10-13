-- CODEOWNERS(T-206):分支保护加"必须所有者评审"开关;PR 评审人指派表
ALTER TABLE rev_branch_protection
    ADD COLUMN IF NOT EXISTS require_code_owner BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE IF NOT EXISTS rev_pr_reviewer (
    id          UUID PRIMARY KEY,
    pr_id       UUID        NOT NULL REFERENCES rev_pull_request (id),
    reviewer    UUID        NOT NULL REFERENCES org_user (id),
    added_by    UUID        REFERENCES org_user (id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (pr_id, reviewer)
);
CREATE INDEX IF NOT EXISTS idx_rev_pr_reviewer_pr
    ON rev_pr_reviewer (pr_id);
