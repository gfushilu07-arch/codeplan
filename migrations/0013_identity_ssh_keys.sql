-- SSH 公钥(T-201,前缀 org_*)
CREATE TABLE IF NOT EXISTS org_ssh_key (
    id           UUID PRIMARY KEY,
    user_id      UUID         NOT NULL REFERENCES org_user (id),
    title        VARCHAR(128) NOT NULL,
    public_key   TEXT         NOT NULL,
    fingerprint  VARCHAR(128) NOT NULL UNIQUE,
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_org_ssh_key_user ON org_ssh_key (user_id);
