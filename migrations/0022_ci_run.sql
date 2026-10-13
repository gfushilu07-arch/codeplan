-- CI 流水线(T-303,前缀 ci_*):workflow 存储 + 运行 + 步骤
CREATE TABLE IF NOT EXISTS ci_pipeline (
    id          UUID PRIMARY KEY,
    repo_id     UUID         NOT NULL REFERENCES repo_repository (id),
    name        VARCHAR(128) NOT NULL,
    yaml        TEXT         NOT NULL,
    parsed      JSONB        NOT NULL DEFAULT '{}',
    enabled     BOOLEAN      NOT NULL DEFAULT TRUE,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    UNIQUE (repo_id, name)
);

CREATE TABLE IF NOT EXISTS ci_run (
    id            UUID PRIMARY KEY,
    pipeline_id   UUID         NOT NULL REFERENCES ci_pipeline (id),
    repo_id       UUID         NOT NULL,
    branch        VARCHAR(255) NOT NULL DEFAULT '',
    sha           VARCHAR(64)  NOT NULL DEFAULT '',
    status        VARCHAR(16)  NOT NULL DEFAULT 'queued',
    trigger_kind  VARCHAR(16)  NOT NULL DEFAULT 'manual',
    actor         UUID         REFERENCES org_user (id),
    started_at    TIMESTAMPTZ,
    finished_at   TIMESTAMPTZ,
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_ci_run_repo ON ci_run (repo_id, created_at DESC);

CREATE TABLE IF NOT EXISTS ci_run_step (
    id          UUID PRIMARY KEY,
    run_id      UUID         NOT NULL REFERENCES ci_run (id) ON DELETE CASCADE,
    job_id      VARCHAR(128) NOT NULL,
    combo       VARCHAR(128) NOT NULL DEFAULT '',
    seq         INT          NOT NULL DEFAULT 0,
    name        VARCHAR(255) NOT NULL DEFAULT '',
    status      VARCHAR(16)  NOT NULL DEFAULT 'pending',
    exit_code   INT,
    output      TEXT         NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_ci_step_run ON ci_run_step (run_id, job_id, seq);
