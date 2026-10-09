-- Wiki(T-115,前缀 wiki_*)
CREATE TABLE IF NOT EXISTS wiki_page (
    id          UUID PRIMARY KEY,
    project_id  UUID         NOT NULL REFERENCES proj_project (id),
    slug        VARCHAR(128) NOT NULL,
    title       VARCHAR(256) NOT NULL,
    content     TEXT         NOT NULL DEFAULT '',
    parent_id   UUID         REFERENCES wiki_page (id),
    updated_by  UUID         REFERENCES org_user (id),
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    UNIQUE (project_id, slug)
);

CREATE TABLE IF NOT EXISTS wiki_attachment (
    id            UUID PRIMARY KEY,
    page_id       UUID         NOT NULL REFERENCES wiki_page (id) ON DELETE CASCADE,
    filename      VARCHAR(255) NOT NULL,
    size_bytes    BIGINT       NOT NULL,
    content_type  VARCHAR(128),
    storage_key   VARCHAR(255) NOT NULL UNIQUE,
    uploaded_by   UUID         REFERENCES org_user (id),
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT now()
);
