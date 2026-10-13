-- 域名绑定(T-315 / PAAS-05):自动分配 + 自定义域名,供网关路由生成
CREATE TABLE IF NOT EXISTS app_domain (
    id          UUID PRIMARY KEY,
    env_id      UUID         NOT NULL REFERENCES app_environment (id),
    host        VARCHAR(255) NOT NULL UNIQUE,
    kind        VARCHAR(16)  NOT NULL DEFAULT 'custom',
    tls_enabled BOOLEAN      NOT NULL DEFAULT TRUE,
    cert_ref    VARCHAR(255) NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_app_domain_env ON app_domain (env_id);
