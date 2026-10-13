-- 访问凭证(T-210,identity 域,前缀 org_*)
-- 个人访问令牌(只存 SHA-256 哈希;明文仅签发时返回一次)
CREATE TABLE IF NOT EXISTS org_personal_access_token (
    id           UUID PRIMARY KEY,
    user_id      UUID         NOT NULL REFERENCES org_user (id),
    name         VARCHAR(128) NOT NULL,
    token_hash   VARCHAR(64)  NOT NULL UNIQUE,
    prefix       VARCHAR(16)  NOT NULL DEFAULT '',
    scopes       TEXT[]       NOT NULL DEFAULT '{}',
    expires_at   TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ,
    revoked_at   TIMESTAMPTZ,
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_org_pat_user ON org_personal_access_token (user_id);

-- 用户 GPG 公钥(提交签名验证,T-210)
CREATE TABLE IF NOT EXISTS org_user_gpg_key (
    id           UUID PRIMARY KEY,
    user_id      UUID         NOT NULL REFERENCES org_user (id),
    fingerprint  VARCHAR(64)  NOT NULL,
    public_key   TEXT         NOT NULL,
    email        VARCHAR(255) NOT NULL DEFAULT '',
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT now(),
    UNIQUE (fingerprint)
);
CREATE INDEX IF NOT EXISTS idx_org_gpg_user ON org_user_gpg_key (user_id);
