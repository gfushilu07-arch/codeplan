-- 部署主机(T-311 / PAAS-03):agent 主动外连注册,平台侧登记与在线状态
CREATE TABLE IF NOT EXISTS app_deploy_host (
    id                UUID PRIMARY KEY,
    name              VARCHAR(64)  NOT NULL UNIQUE,
    token_hash        VARCHAR(128) NOT NULL,
    registered        BOOLEAN      NOT NULL DEFAULT FALSE,
    hostname          VARCHAR(255) NOT NULL DEFAULT '',
    agent_version     VARCHAR(32)  NOT NULL DEFAULT '',
    labels            JSONB        NOT NULL DEFAULT '[]'::jsonb,
    status            VARCHAR(16)  NOT NULL DEFAULT 'pending',
    last_heartbeat_at TIMESTAMPTZ,
    created_at        TIMESTAMPTZ  NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_app_host_status ON app_deploy_host (status, last_heartbeat_at DESC);
