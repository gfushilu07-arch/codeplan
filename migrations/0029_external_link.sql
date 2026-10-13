-- 外部链接器(T-308 / MS-03):工作项/PR/发布页跳转既有系统(Grafana/Jaeger/SonarQube…)
CREATE TABLE IF NOT EXISTS ins_external_link (
    id           UUID PRIMARY KEY,
    scope_kind   VARCHAR(16)  NOT NULL DEFAULT 'global',
    scope_id     UUID,
    name         VARCHAR(128) NOT NULL,
    url_template TEXT         NOT NULL,
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT now()
);
-- scope_id 可空,用 COALESCE 保证同名唯一(纯 NULL 唯一索引不去重)
CREATE UNIQUE INDEX IF NOT EXISTS uq_ins_external_link
    ON ins_external_link (scope_kind, COALESCE(scope_id, '00000000-0000-0000-0000-000000000000'::uuid), name);
