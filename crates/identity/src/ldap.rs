//! LDAP/AD 目录对接(ORG-01):认证回退 + 组织架构定时同步。
//!
//! 配置化启用(`CODEPLAN_LDAP_URL` 等环境变量),未配置即未启用。
//! 测试经 [`LdapDirectory`] trait 注入内存目录。

use async_trait::async_trait;
use codeplan_common::error::AppError;
use ldap3::{LdapConnAsync, Scope, SearchEntry};
use serde::{Deserialize, Serialize};

use crate::DOMAIN_CODE;

/// LDAP 用户(目录侧信息)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LdapUser {
    /// 登录名(目录 uid / sAMAccountName)。
    pub username: String,
    /// 邮箱。
    pub email: String,
    /// 显示名(cn)。
    pub display_name: String,
}

/// 目录抽象:认证 + 全量用户清单(生产实现为 ldap3,测试为内存目录)。
#[async_trait]
pub trait LdapDirectory: Send + Sync {
    /// 校验目录账号密码;成功返回用户信息,失败返回 `Ok(None)`。
    ///
    /// # Errors
    /// 目录连接/配置故障返回错误(与"认证失败"区分)。
    async fn authenticate(
        &self,
        username: &str,
        password: &str,
    ) -> Result<Option<LdapUser>, String>;

    /// 全量用户清单(同步用)。
    ///
    /// # Errors
    /// 目录连接/配置故障。
    async fn list_users(&self) -> Result<Vec<LdapUser>, String>;
}

/// LDAP 配置。
#[derive(Debug, Clone)]
pub struct LdapConfig {
    /// 连接串(如 `ldap://ad.corp:389`)。
    pub url: String,
    /// 服务账号 DN(只读检索用)。
    pub bind_dn: String,
    /// 服务账号密码。
    pub bind_password: String,
    /// 用户检索基准 DN。
    pub user_base_dn: String,
    /// 用户过滤模板,`{username}` 占位。
    pub user_filter: String,
    /// uid 属性名(AD 常为 sAMAccountName)。
    pub attr_uid: String,
    /// mail 属性名。
    pub attr_mail: String,
    /// cn 属性名。
    pub attr_cn: String,
}

impl LdapConfig {
    /// 从环境变量读取;`CODEPLAN_LDAP_URL` 缺失即未启用。
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let url = std::env::var("CODEPLAN_LDAP_URL").ok()?;
        Some(Self {
            url,
            bind_dn: std::env::var("CODEPLAN_LDAP_BIND_DN").unwrap_or_default(),
            bind_password: std::env::var("CODEPLAN_LDAP_BIND_PASSWORD").unwrap_or_default(),
            user_base_dn: std::env::var("CODEPLAN_LDAP_USER_BASE_DN").unwrap_or_default(),
            user_filter: std::env::var("CODEPLAN_LDAP_USER_FILTER")
                .unwrap_or_else(|_| "(uid={username})".to_owned()),
            attr_uid: std::env::var("CODEPLAN_LDAP_ATTR_UID").unwrap_or_else(|_| "uid".to_owned()),
            attr_mail: std::env::var("CODEPLAN_LDAP_ATTR_MAIL")
                .unwrap_or_else(|_| "mail".to_owned()),
            attr_cn: std::env::var("CODEPLAN_LDAP_ATTR_CN").unwrap_or_else(|_| "cn".to_owned()),
        })
    }
}

/// ldap3 实现。
pub struct LdapDirectoryImpl {
    config: LdapConfig,
}

impl LdapDirectoryImpl {
    /// 构造。
    #[must_use]
    pub fn new(config: LdapConfig) -> Self {
        Self { config }
    }

    async fn connect(&self) -> Result<ldap3::Ldap, String> {
        let (conn, mut ldap) = LdapConnAsync::new(&self.config.url)
            .await
            .map_err(|e| e.to_string())?;
        ldap3::drive!(conn);
        if !self.config.bind_dn.is_empty() {
            ldap.simple_bind(&self.config.bind_dn, &self.config.bind_password)
                .await
                .map_err(|e| e.to_string())?
                .success()
                .map_err(|e| e.to_string())?;
        }
        Ok(ldap)
    }

    async fn search_users(&self, filter: &str) -> Result<Vec<LdapUser>, String> {
        let mut ldap = self.connect().await?;
        let attrs = vec![
            self.config.attr_uid.clone(),
            self.config.attr_mail.clone(),
            self.config.attr_cn.clone(),
        ];
        let (entries, _res) = ldap
            .search(&self.config.user_base_dn, Scope::Subtree, filter, attrs)
            .await
            .map_err(|e| e.to_string())?
            .success()
            .map_err(|e| e.to_string())?;
        let users = entries
            .into_iter()
            .map(SearchEntry::construct)
            .filter_map(|e| {
                let first = |name: &str| e.attrs.get(name).and_then(|v| v.first().cloned());
                let username = first(&self.config.attr_uid)?;
                Some(LdapUser {
                    username: username.clone(),
                    email: first(&self.config.attr_mail)
                        .unwrap_or_else(|| format!("{username}@ldap")),
                    display_name: first(&self.config.attr_cn).unwrap_or(username),
                })
            })
            .collect();
        let _ = ldap.unbind().await;
        Ok(users)
    }
}

#[async_trait]
impl LdapDirectory for LdapDirectoryImpl {
    async fn authenticate(
        &self,
        username: &str,
        password: &str,
    ) -> Result<Option<LdapUser>, String> {
        let filter = self.config.user_filter.replace("{username}", username);
        let users = self.search_users(&filter).await?;
        let Some(user) = users.into_iter().next() else {
            return Ok(None);
        };

        // 以用户 DN 绑定验证密码:先检索 DN——search_users 丢了 entry dn,
        // 因此单独按过滤器检索 DN(简化:再次 search 取 dn)
        let mut ldap = self.connect().await?;
        let (entries, _res) = ldap
            .search(
                &self.config.user_base_dn,
                Scope::Subtree,
                &filter,
                vec!["dn".to_owned()],
            )
            .await
            .map_err(|e| e.to_string())?
            .success()
            .map_err(|e| e.to_string())?;
        let Some(entry) = entries.into_iter().next() else {
            let _ = ldap.unbind().await;
            return Ok(None);
        };
        let dn = SearchEntry::construct(entry).dn;
        let bind = ldap
            .simple_bind(&dn, password)
            .await
            .map_err(|e| e.to_string())?;
        let ok = bind.success().is_ok();
        let _ = ldap.unbind().await;
        Ok(ok.then_some(user))
    }

    async fn list_users(&self) -> Result<Vec<LdapUser>, String> {
        let filter = self.config.user_filter.replace("{username}", "*");
        // `(uid=*)` 形式的通配过滤;若模板为 `(uid={username})` 替换后即为通配
        self.search_users(&filter).await
    }
}

/// 上抛目录故障为领域错误(503:目录不可用,与 401 认证失败区分)。
#[must_use]
pub fn directory_error(e: String) -> AppError {
    AppError::new(format!("{DOMAIN_CODE}-503"), 503, "目录服务不可用")
        .with_details(serde_json::json!(e))
}
