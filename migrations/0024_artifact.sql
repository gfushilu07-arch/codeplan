-- 制品库(T-304,前缀 reg_*):通用制品元数据;内容在对象存储 artifacts/
CREATE TABLE IF NOT EXISTS reg_artifact (
    id          UUID PRIMARY KEY,
    repo_id     UUID         NOT NULL REFERENCES repo_repository (id),
    kind        VARCHAR(16)  NOT NULL,   -- maven/npm/pypi/oci/generic
    name        VARCHAR(255) NOT NULL,   -- maven=group:artifact / npm=包名 / pypi=规范化名 / oci=repository
    version     VARCHAR(128) NOT NULL,
    filename    VARCHAR(255) NOT NULL,
    size        BIGINT       NOT NULL DEFAULT 0,
    sha256      VARCHAR(64)  NOT NULL DEFAULT '',
    meta        JSONB        NOT NULL DEFAULT '{}',
    created_by  UUID         REFERENCES org_user (id),
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    UNIQUE (repo_id, kind, name, version, filename)
);
CREATE INDEX IF NOT EXISTS idx_reg_artifact_lookup
    ON reg_artifact (repo_id, kind, name);
