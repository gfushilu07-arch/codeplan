-- 分支保护规则(T-205,前缀 rev_*)
CREATE TABLE IF NOT EXISTS rev_branch_protection (
    id                UUID PRIMARY KEY,
    repo_id           UUID         NOT NULL REFERENCES repo_repository (id),
    branch_pattern    VARCHAR(255) NOT NULL,
    min_approvals     INT          NOT NULL DEFAULT 0,
    required_contexts TEXT[]       NOT NULL DEFAULT '{}',
    block_force_push  BOOLEAN      NOT NULL DEFAULT TRUE,
    block_direct_push BOOLEAN      NOT NULL DEFAULT FALSE,
    created_at        TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ  NOT NULL DEFAULT now(),
    UNIQUE (repo_id, branch_pattern)
);
