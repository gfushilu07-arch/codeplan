-- 应用中心(T-310 / PAAS-01,02):应用一等实体 + 环境 + 部署目标 + 变量 + 仓库聚合
CREATE TABLE IF NOT EXISTS app_application (
    id          UUID PRIMARY KEY,
    key         VARCHAR(64)  NOT NULL UNIQUE,
    name        VARCHAR(128) NOT NULL,
    description TEXT         NOT NULL DEFAULT '',
    owner_id    UUID,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS app_environment (
    id         UUID PRIMARY KEY,
    app_id     UUID        NOT NULL REFERENCES app_application (id),
    name       VARCHAR(64) NOT NULL,
    kind       VARCHAR(16) NOT NULL DEFAULT 'dev',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (app_id, name)
);

-- 部署目标(主机群登记;T-311 agent 注册后回填 agent 侧标识)
CREATE TABLE IF NOT EXISTS app_deploy_target (
    id         UUID PRIMARY KEY,
    env_id     UUID         NOT NULL REFERENCES app_environment (id),
    name       VARCHAR(64)  NOT NULL,
    host       VARCHAR(255) NOT NULL,
    port       INTEGER      NOT NULL DEFAULT 22,
    labels     JSONB        NOT NULL DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ  NOT NULL DEFAULT now(),
    UNIQUE (env_id, name)
);

-- 环境变量注入(敏感项标记 is_secret,值不随普通查询返回)
CREATE TABLE IF NOT EXISTS app_env_var (
    id         UUID PRIMARY KEY,
    env_id     UUID         NOT NULL REFERENCES app_environment (id),
    key        VARCHAR(128) NOT NULL,
    value      TEXT         NOT NULL DEFAULT '',
    is_secret  BOOLEAN      NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ  NOT NULL DEFAULT now(),
    UNIQUE (env_id, key)
);

-- 应用 ↔ 仓库聚合(跨域仅存 ID,名称经只读投影取)
CREATE TABLE IF NOT EXISTS app_repo_link (
    app_id   UUID NOT NULL REFERENCES app_application (id),
    repo_id  UUID NOT NULL,
    added_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (app_id, repo_id)
);

CREATE INDEX IF NOT EXISTS idx_app_env_app ON app_environment (app_id);
CREATE INDEX IF NOT EXISTS idx_app_target_env ON app_deploy_target (env_id);
