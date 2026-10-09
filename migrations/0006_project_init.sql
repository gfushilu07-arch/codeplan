-- 项目管理域(T-111,前缀 proj_*)
CREATE TABLE IF NOT EXISTS proj_project (
    id           UUID PRIMARY KEY,
    key          VARCHAR(16)  NOT NULL UNIQUE,
    name         VARCHAR(128) NOT NULL,
    description  TEXT         NOT NULL DEFAULT '',
    lead         UUID REFERENCES org_user (id),
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS proj_work_item (
    id             UUID PRIMARY KEY,
    project_id     UUID        NOT NULL REFERENCES proj_project (id),
    item_number    BIGINT      NOT NULL,
    item_type      VARCHAR(16) NOT NULL,
    title          VARCHAR(256) NOT NULL,
    description    TEXT        NOT NULL DEFAULT '',
    status         VARCHAR(32) NOT NULL,
    priority       VARCHAR(16) NOT NULL DEFAULT 'medium',
    assignee       UUID REFERENCES org_user (id),
    reporter       UUID REFERENCES org_user (id),
    custom_fields  JSONB       NOT NULL DEFAULT '{}',
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, item_number)
);
CREATE INDEX IF NOT EXISTS idx_proj_item_project ON proj_work_item (project_id, status);

-- 可配置工作流(缺省回落到内置状态机)
CREATE TABLE IF NOT EXISTS proj_workflow (
    project_id  UUID        NOT NULL REFERENCES proj_project (id),
    item_type   VARCHAR(16) NOT NULL,
    config      JSONB       NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, item_type)
);
