-- LDAP 同步来源标记 + 审计日志(T-105 / T-106)
ALTER TABLE org_user ADD COLUMN IF NOT EXISTS source VARCHAR(16) NOT NULL DEFAULT 'local';
-- source: local | ldap | oidc

CREATE TABLE IF NOT EXISTS org_audit_log (
    id             UUID PRIMARY KEY,
    actor          UUID REFERENCES org_user (id),
    action         VARCHAR(64)  NOT NULL,
    resource_kind  VARCHAR(16),
    resource_id    UUID,
    detail         JSONB        NOT NULL DEFAULT '{}',
    at             TIMESTAMPTZ  NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_org_audit_at ON org_audit_log (at DESC);
