-- 部署执行(T-313 / PAAS-04):部署记录 + 命令回执路由
CREATE TABLE IF NOT EXISTS app_deployment (
    id           UUID PRIMARY KEY,
    env_id       UUID        NOT NULL REFERENCES app_environment (id),
    app_key      VARCHAR(64) NOT NULL,
    spec_version INTEGER,
    status       VARCHAR(16) NOT NULL DEFAULT 'pending',
    target_hosts JSONB       NOT NULL DEFAULT '[]'::jsonb,
    results      JSONB       NOT NULL DEFAULT '{}'::jsonb,
    detail       TEXT        NOT NULL DEFAULT '',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at  TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_app_deploy_env ON app_deployment (env_id, created_at DESC);

-- 命令 → 部署 映射:agent 回执按 command_id 路由回部署记录
CREATE TABLE IF NOT EXISTS app_command (
    id            UUID PRIMARY KEY,
    deployment_id UUID        NOT NULL REFERENCES app_deployment (id),
    host_id       UUID        NOT NULL,
    kind          VARCHAR(24) NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_app_command_deploy ON app_command (deployment_id);
