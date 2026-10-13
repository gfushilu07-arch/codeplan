-- 评审流(T-204,前缀 rev_*):
-- 1) PR 评论(对话流 + 行内锚定 diff)
CREATE TABLE IF NOT EXISTS rev_pr_comment (
    id          UUID PRIMARY KEY,
    pr_id       UUID         NOT NULL REFERENCES rev_pull_request (id),
    author      UUID         NOT NULL REFERENCES org_user (id),
    body        TEXT         NOT NULL,
    file_path   VARCHAR(512),
    line        INT,
    side        VARCHAR(8),
    anchor_sha  VARCHAR(64),
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_rev_pr_comment_pr
    ON rev_pr_comment (pr_id, created_at);

-- 2) 评审记录(通过/驳回/仅评论;保留历史,取每人最新一条为当前立场)
CREATE TABLE IF NOT EXISTS rev_pr_review (
    id          UUID PRIMARY KEY,
    pr_id       UUID         NOT NULL REFERENCES rev_pull_request (id),
    reviewer    UUID         NOT NULL REFERENCES org_user (id),
    verdict     VARCHAR(24)  NOT NULL,
    body        TEXT         NOT NULL DEFAULT '',
    head_sha    VARCHAR(64),
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_rev_pr_review_pr
    ON rev_pr_review (pr_id, created_at DESC);

-- 3) 提交状态检查(CI 等外部系统按 context 上报,同 context 覆盖更新)
CREATE TABLE IF NOT EXISTS rev_commit_status (
    id          UUID PRIMARY KEY,
    repo_id     UUID         NOT NULL REFERENCES repo_repository (id),
    commit_sha  VARCHAR(64)  NOT NULL,
    context     VARCHAR(128) NOT NULL,
    state       VARCHAR(16)  NOT NULL,
    description VARCHAR(255) NOT NULL DEFAULT '',
    target_url  TEXT,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    UNIQUE (repo_id, commit_sha, context)
);
CREATE INDEX IF NOT EXISTS idx_rev_commit_status_lookup
    ON rev_commit_status (repo_id, commit_sha);
