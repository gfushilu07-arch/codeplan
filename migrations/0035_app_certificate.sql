-- 证书(T-316 / PAAS-07):ACME 签发结果,回填 app_domain.cert_ref 供网关启用 TLS
CREATE TABLE IF NOT EXISTS app_certificate (
    id         UUID PRIMARY KEY,
    env_id     UUID         NOT NULL REFERENCES app_environment (id),
    cert_ref   VARCHAR(128) NOT NULL UNIQUE,
    domains    JSONB        NOT NULL,
    provider   VARCHAR(32)  NOT NULL DEFAULT 'cert-acme',
    status     VARCHAR(16)  NOT NULL DEFAULT 'issued',
    not_after  TIMESTAMPTZ,
    issued_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    created_at TIMESTAMPTZ  NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_app_cert_env ON app_certificate (env_id, issued_at DESC);
CREATE INDEX IF NOT EXISTS idx_app_cert_expiry ON app_certificate (not_after);
