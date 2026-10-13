-- Pull Request(T-203,评审域;前缀 rev_*)
CREATE TABLE IF NOT EXISTS rev_pull_request (
    id             UUID PRIMARY KEY,
    repo_id        UUID         NOT NULL REFERENCES repo_repository (id),
    number         BIGINT       NOT NULL,
    title          VARCHAR(255) NOT NULL,
    body           TEXT         NOT NULL DEFAULT '',
    source_branch  VARCHAR(255) NOT NULL,
    target_branch  VARCHAR(255) NOT NULL,
    author         UUID         NOT NULL REFERENCES org_user (id),
    state          VARCHAR(16)  NOT NULL DEFAULT 'open',
    is_draft       BOOLEAN      NOT NULL DEFAULT FALSE,
    merge_method   VARCHAR(16),
    merge_commit   VARCHAR(64),
    merged_by      UUID         REFERENCES org_user (id),
    merged_at      TIMESTAMPTZ,
    created_at     TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ  NOT NULL DEFAULT now(),
    UNIQUE (repo_id, number)
);

CREATE INDEX IF NOT EXISTS idx_rev_pr_repo_state
    ON rev_pull_request (repo_id, state, number DESC);
