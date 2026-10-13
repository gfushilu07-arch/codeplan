-- 配置与密钥(T-317 / PAAS-06):按环境管理配置,版本化留痕,敏感项密文落库
CREATE TABLE IF NOT EXISTS app_config (
    id              UUID PRIMARY KEY,
    app_id          UUID        NOT NULL REFERENCES app_application (id),
    env_id          UUID        NOT NULL REFERENCES app_environment (id),
    current_version INTEGER     NOT NULL DEFAULT 0,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (app_id, env_id)
);

CREATE TABLE IF NOT EXISTS app_config_version (
    id         UUID PRIMARY KEY,
    config_id  UUID        NOT NULL REFERENCES app_config (id),
    version    INTEGER     NOT NULL,
    items      JSONB       NOT NULL,
    source     TEXT        NOT NULL DEFAULT '',
    created_by UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (config_id, version)
);
CREATE INDEX IF NOT EXISTS idx_app_config_ver ON app_config_version (config_id, version DESC);
