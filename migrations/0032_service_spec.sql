-- 服务规格(T-312 / PAAS-03):声明式、版本化、可评审
CREATE TABLE IF NOT EXISTS app_service (
    id              UUID PRIMARY KEY,
    app_id          UUID        NOT NULL REFERENCES app_application (id),
    env_id          UUID        NOT NULL REFERENCES app_environment (id),
    current_version INTEGER     NOT NULL DEFAULT 0,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (app_id, env_id)
);

CREATE TABLE IF NOT EXISTS app_service_spec (
    id         UUID PRIMARY KEY,
    service_id UUID        NOT NULL REFERENCES app_service (id),
    version    INTEGER     NOT NULL,
    spec       JSONB       NOT NULL,
    source     TEXT        NOT NULL DEFAULT '',
    created_by UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (service_id, version)
);
CREATE INDEX IF NOT EXISTS idx_app_spec_service ON app_service_spec (service_id, version DESC);
