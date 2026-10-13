-- Tag/Release(T-208,前缀 repo_*;附件内容在对象存储 <data>/release-assets)
CREATE TABLE IF NOT EXISTS repo_release (
    id          UUID PRIMARY KEY,
    repo_id     UUID         NOT NULL REFERENCES repo_repository (id),
    tag_name    VARCHAR(128) NOT NULL,
    name        VARCHAR(255) NOT NULL DEFAULT '',
    body        TEXT         NOT NULL DEFAULT '',
    notes_auto  BOOLEAN      NOT NULL DEFAULT FALSE,
    author      UUID         NOT NULL REFERENCES org_user (id),
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    UNIQUE (repo_id, tag_name)
);

CREATE TABLE IF NOT EXISTS repo_release_asset (
    id          UUID PRIMARY KEY,
    release_id  UUID         NOT NULL REFERENCES repo_release (id) ON DELETE CASCADE,
    filename    VARCHAR(255) NOT NULL,
    size        BIGINT       NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    UNIQUE (release_id, filename)
);
