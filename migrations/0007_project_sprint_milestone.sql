-- 迭代与里程碑(T-112)
CREATE TABLE IF NOT EXISTS proj_sprint (
    id              UUID PRIMARY KEY,
    project_id      UUID        NOT NULL REFERENCES proj_project (id),
    name            VARCHAR(128) NOT NULL,
    goal            TEXT        NOT NULL DEFAULT '',
    starts_at       TIMESTAMPTZ,
    ends_at         TIMESTAMPTZ,
    capacity_points INTEGER     NOT NULL DEFAULT 0,
    status          VARCHAR(16) NOT NULL DEFAULT 'planned',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_proj_sprint_project ON proj_sprint (project_id);

CREATE TABLE IF NOT EXISTS proj_milestone (
    id          UUID PRIMARY KEY,
    project_id  UUID        NOT NULL REFERENCES proj_project (id),
    name        VARCHAR(128) NOT NULL,
    description TEXT        NOT NULL DEFAULT '',
    due_date    DATE,
    status      VARCHAR(16) NOT NULL DEFAULT 'open',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_proj_milestone_project ON proj_milestone (project_id);

ALTER TABLE proj_work_item ADD COLUMN IF NOT EXISTS sprint_id UUID REFERENCES proj_sprint (id);
ALTER TABLE proj_work_item ADD COLUMN IF NOT EXISTS milestone_id UUID REFERENCES proj_milestone (id);
