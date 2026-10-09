-- 身份域初始表(前缀 org_*,所有权:identity 域)
CREATE TABLE IF NOT EXISTS org_user (
    id                 UUID PRIMARY KEY,
    username           VARCHAR(64)  NOT NULL UNIQUE,
    email              VARCHAR(255) NOT NULL UNIQUE,
    display_name       VARCHAR(128) NOT NULL,
    status             VARCHAR(16)  NOT NULL DEFAULT 'active',
    password_hash      VARCHAR(255),
    is_platform_admin  BOOLEAN      NOT NULL DEFAULT FALSE,
    created_at         TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS org_organization (
    id          UUID PRIMARY KEY,
    name        VARCHAR(128) NOT NULL UNIQUE,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS org_team (
    id          UUID PRIMARY KEY,
    org_id      UUID NOT NULL REFERENCES org_organization (id),
    name        VARCHAR(128) NOT NULL,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS org_team_member (
    team_id    UUID NOT NULL REFERENCES org_team (id),
    user_id    UUID NOT NULL REFERENCES org_user (id),
    joined_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (team_id, user_id)
);
