//! 外部链接器(T-308 / MS-03):把工作项/PR/发布页跳转到既有系统。
//!
//! 链接以**模板**声明(`https://grafana.internal/d/ci?var-repo={repo}&var-sha={sha}`),
//! 解析时按当前上下文替换占位符;无法替换的占位符原样保留(便于发现配置错误)。

use std::collections::HashMap;

use codeplan_common::error::AppError;
use serde::Serialize;

/// 域错误码前缀。
const DOMAIN_CODE: &str = "MS";

/// 生效范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkScope {
    /// 全平台。
    Global,
    /// 单仓库。
    Repo,
    /// 单项目。
    Project,
}

impl LinkScope {
    /// 入库值。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Repo => "repo",
            Self::Project => "project",
        }
    }

    /// 解析。
    #[must_use]
    pub fn parse(v: &str) -> Option<Self> {
        match v {
            "global" => Some(Self::Global),
            "repo" => Some(Self::Repo),
            "project" => Some(Self::Project),
            _ => None,
        }
    }
}

/// 链接定义。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExternalLink {
    /// ID。
    pub id: uuid::Uuid,
    /// 范围。
    pub scope_kind: LinkScope,
    /// 范围对象(global 为空)。
    pub scope_id: Option<uuid::Uuid>,
    /// 展示名。
    pub name: String,
    /// URL 模板。
    pub url_template: String,
}

/// 解析后的链接(模板已替换)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedLink {
    /// 展示名。
    pub name: String,
    /// 最终 URL。
    pub url: String,
}

/// 支持的占位符。
pub const PLACEHOLDERS: &[&str] = &[
    "repo",
    "sha",
    "branch",
    "item_number",
    "pr_number",
    "tag",
];

/// 用上下文替换模板占位符(未知占位符原样保留)。
#[must_use]
pub fn render(template: &str, ctx: &HashMap<String, String>) -> String {
    let mut out = template.to_owned();
    for key in PLACEHOLDERS {
        if let Some(value) = ctx.get(*key) {
            out = out.replace(&format!("{{{key}}}"), value);
        }
    }
    out
}

/// 模板是否引用了合法占位符之外的花括号变量。
#[must_use]
pub fn unknown_placeholders(template: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else { break };
        let name = &after[..end];
        if !PLACEHOLDERS.contains(&name) && !found.iter().any(|f| f == name) {
            found.push(name.to_owned());
        }
        rest = &after[end + 1..];
    }
    found
}

/// 外部链接存储。
pub struct PgLinkStore {
    pool: sqlx::PgPool,
}

impl PgLinkStore {
    /// 构造。
    #[must_use]
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    /// 新增(同名同范围覆盖更新)。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn upsert(
        &self,
        scope: LinkScope,
        scope_id: Option<uuid::Uuid>,
        name: &str,
        url_template: &str,
    ) -> Result<uuid::Uuid, AppError> {
        let id: (uuid::Uuid,) = sqlx::query_as(
            "INSERT INTO ins_external_link (id, scope_kind, scope_id, name, url_template) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (scope_kind, COALESCE(scope_id, '00000000-0000-0000-0000-000000000000'::uuid), name) \
             DO UPDATE SET url_template = EXCLUDED.url_template \
             RETURNING id",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(scope.as_str())
        .bind(scope_id)
        .bind(name)
        .bind(url_template)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string())))?;
        Ok(id.0)
    }

    /// 按范围列出(含全平台链接;具体范围优先)。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn list_for(
        &self,
        scope: LinkScope,
        scope_id: Option<uuid::Uuid>,
    ) -> Result<Vec<ExternalLink>, AppError> {
        let rows: Vec<(uuid::Uuid, String, Option<uuid::Uuid>, String, String)> = sqlx::query_as(
            "SELECT id, scope_kind, scope_id, name, url_template FROM ins_external_link \
             WHERE scope_kind = 'global' OR (scope_kind = $1 AND scope_id = $2) \
             ORDER BY CASE scope_kind WHEN 'global' THEN 1 ELSE 0 END, name",
        )
        .bind(scope.as_str())
        .bind(scope_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string())))?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, kind, scope_id, name, url_template)| {
                Some(ExternalLink {
                    id,
                    scope_kind: LinkScope::parse(&kind)?,
                    scope_id,
                    name,
                    url_template,
                })
            })
            .collect())
    }

    /// 删除。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn delete(&self, id: uuid::Uuid) -> Result<bool, AppError> {
        let res = sqlx::query("DELETE FROM ins_external_link WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string())))?;
        Ok(res.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn render_replaces_known_placeholders() {
        let t = "https://grafana.internal/d/ci?var-repo={repo}&var-sha={sha}&var-branch={branch}";
        let out = render(t, &ctx(&[("repo", "admin/demo"), ("sha", "abc123"), ("branch", "main")]));
        assert_eq!(
            out,
            "https://grafana.internal/d/ci?var-repo=admin/demo&var-sha=abc123&var-branch=main"
        );
    }

    #[test]
    fn render_keeps_unknown_placeholder_visible() {
        let out = render("https://x/{repo}/{mystery}", &ctx(&[("repo", "a/b")]));
        assert_eq!(out, "https://x/a/b/{mystery}", "未知占位符原样保留便于排查");
    }

    #[test]
    fn render_without_context_is_unchanged() {
        let t = "https://sonar.internal/dashboard?id={repo}";
        assert_eq!(render(t, &HashMap::new()), t);
    }

    #[test]
    fn detects_unknown_placeholders() {
        let found = unknown_placeholders("https://x/{repo}/{foo}/{bar}/{repo}");
        assert_eq!(found, vec!["foo".to_owned(), "bar".to_owned()], "去重且只报未知项");
        assert!(unknown_placeholders("https://x/{repo}/{sha}").is_empty());
    }

    #[test]
    fn scope_roundtrip() {
        for s in [LinkScope::Global, LinkScope::Repo, LinkScope::Project] {
            assert_eq!(LinkScope::parse(s.as_str()), Some(s));
        }
        assert_eq!(LinkScope::parse("nope"), None);
    }
}
