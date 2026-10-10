-- 站内通知(T-116,前缀 ntf_*)
CREATE TABLE IF NOT EXISTS ntf_notification (
    id          UUID PRIMARY KEY,
    recipient   UUID        NOT NULL REFERENCES org_user (id),
    title       VARCHAR(256) NOT NULL,
    body        TEXT        NOT NULL DEFAULT '',
    link        VARCHAR(255),
    channel     VARCHAR(32)  NOT NULL DEFAULT 'inbox',
    read_at     TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_ntf_recipient ON ntf_notification (recipient, created_at DESC);
