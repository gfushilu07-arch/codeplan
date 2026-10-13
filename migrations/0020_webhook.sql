-- Webhook 出站(T-209,前缀 repo_*)
CREATE TABLE IF NOT EXISTS repo_webhook (
    id          UUID PRIMARY KEY,
    repo_id     UUID         NOT NULL REFERENCES repo_repository (id),
    url         TEXT         NOT NULL,
    secret      VARCHAR(255) NOT NULL,
    events      TEXT[]       NOT NULL DEFAULT '{}',
    active      BOOLEAN      NOT NULL DEFAULT TRUE,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_repo_webhook_repo ON repo_webhook (repo_id);

-- 投递日志(每次投递终态一条;失败含错误与尝试次数)
CREATE TABLE IF NOT EXISTS repo_webhook_delivery (
    id            UUID PRIMARY KEY,
    webhook_id    UUID         NOT NULL REFERENCES repo_webhook (id) ON DELETE CASCADE,
    event_type    VARCHAR(64)  NOT NULL,
    request_body  TEXT         NOT NULL DEFAULT '',
    status_code   INT,
    error         TEXT,
    attempts      INT          NOT NULL DEFAULT 1,
    ok            BOOLEAN      NOT NULL,
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_repo_whd_hook
    ON repo_webhook_delivery (webhook_id, created_at DESC);
