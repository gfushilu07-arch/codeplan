-- 资源级角色(ORG-03):每用户每资源一条,角色取 read/write/admin
-- 系统级管理员沿用 org_user.is_platform_admin;自定义角色引擎随项目域需求引入
CREATE TABLE IF NOT EXISTS org_resource_role (
    user_id        UUID        NOT NULL REFERENCES org_user (id),
    resource_kind  VARCHAR(16) NOT NULL,
    resource_id    UUID        NOT NULL,
    role           VARCHAR(16) NOT NULL,
    granted_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, resource_kind, resource_id)
);
