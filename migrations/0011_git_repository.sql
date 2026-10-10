-- Git 仓库元数据(T-120,前缀 repo_*;裸仓库在文件系统 <data>/repositories)
CREATE TABLE IF NOT EXISTS repo_repository (
    id              UUID PRIMARY KEY,
    full_name       VARCHAR(255) NOT NULL UNIQUE,
    key             VARCHAR(64)  NOT NULL,
    name            VARCHAR(128) NOT NULL,
    description     TEXT         NOT NULL DEFAULT '',
    visibility      VARCHAR(16)  NOT NULL DEFAULT 'private',
    default_branch  VARCHAR(64)  NOT NULL DEFAULT 'main',
    owner           UUID         NOT NULL REFERENCES org_user (id),
    project_id      UUID         REFERENCES proj_project (id),
    archived        BOOLEAN      NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ  NOT NULL DEFAULT now()
);
