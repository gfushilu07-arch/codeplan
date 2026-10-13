//! 应用中心用例:应用/环境/部署目标/变量/仓库聚合,含权限校验与领域事件。

use std::sync::Arc;

use codeplan_common::error::AppError;
use codeplan_kernel::authz::{Action, Authz};
use codeplan_kernel::events::{EventBus, EventEnvelope};
use codeplan_kernel::identity::CurrentUser;
use serde_json::json;

use crate::DOMAIN_CODE;
use codeplan_ext::CertIssuer;

use crate::crypto::ConfigKey;
use crate::domain::{
    AppAggregate, AppBrief, AppCenterRepository, Application, ApplicationId, ConfigItem,
    DomainId, DomainKind, DomainRepository, NewDomain,
    Certificate, CertificateRepository, ConfigItemView, ConfigRepository, ConfigVersion,
    AgentCommandSink, DeployHost, DeployHostId, DeployHostRepository, DeployTarget, DeployTargetId,
    Deployment, DeploymentRepository, EnvBrief,
    EnvKind, EnvVar, EnvVarId, Environment, EnvironmentId, NewApplication, NewDeployTarget,
    NewEnvVar, NewEnvironment, RepoBrief, SpecRepository, SpecVersion, parse_config, parse_spec,
    valid_app_key,
};

/// 应用中心依赖集合(收拢构造函数参数)。
pub struct AppCenterStores {
    /// 应用/环境/目标/变量。
    pub repo: Arc<dyn AppCenterRepository>,
    /// 服务规格(版本化)。
    pub specs: Arc<dyn SpecRepository>,
    /// 配置与密钥(版本化)。
    pub configs: Arc<dyn ConfigRepository>,
    /// 域名绑定。
    pub domains: Arc<dyn DomainRepository>,
    /// 证书记录。
    pub certs: Arc<dyn CertificateRepository>,
    /// 证书签发器(插件;未装配则证书接口返回明确错误)。
    pub issuer: Option<Arc<dyn CertIssuer>>,
    /// 部署主机。
    pub hosts: Arc<dyn DeployHostRepository>,
    /// 部署记录。
    pub deployments: Arc<dyn DeploymentRepository>,
    /// agent 命令投递口(未装配则无法下发部署)。
    pub agent: Option<Arc<dyn AgentCommandSink>>,
    /// 敏感配置主密钥。
    pub key: ConfigKey,
}

/// 应用中心服务。
pub struct AppCenterService {
    repo: Arc<dyn AppCenterRepository>,
    specs: Arc<dyn SpecRepository>,
    configs: Arc<dyn ConfigRepository>,
    domains: Arc<dyn DomainRepository>,
    certs: Arc<dyn CertificateRepository>,
    issuer: Option<Arc<dyn CertIssuer>>,
    hosts: Arc<dyn DeployHostRepository>,
    deployments: Arc<dyn DeploymentRepository>,
    agent: Option<Arc<dyn AgentCommandSink>>,
    key: ConfigKey,
    authz: Arc<dyn Authz>,
    events: Arc<dyn EventBus>,
}

impl AppCenterService {
    /// 构造。
    #[must_use]
    pub fn new(stores: AppCenterStores, authz: Arc<dyn Authz>, events: Arc<dyn EventBus>) -> Self {
        Self {
            repo: stores.repo,
            specs: stores.specs,
            configs: stores.configs,
            domains: stores.domains,
            certs: stores.certs,
            issuer: stores.issuer,
            hosts: stores.hosts,
            deployments: stores.deployments,
            agent: stores.agent,
            key: stores.key,
            authz,
            events,
        }
    }

    /// 建应用(创建者成为负责人;发布 `app.created`,identity 授予 app admin)。
    ///
    /// # Errors
    /// 键非法 `APP-400`;键重复 `APP-409`。
    pub async fn create_app(
        &self,
        actor: &CurrentUser,
        key: &str,
        name: &str,
        description: &str,
    ) -> Result<Application, AppError> {
        let key = key.trim();
        if !valid_app_key(key) {
            return Err(AppError::invalid(
                DOMAIN_CODE,
                "应用键须为 2~40 位小写字母/数字/连字符",
            ));
        }
        if name.trim().is_empty() {
            return Err(AppError::invalid(DOMAIN_CODE, "应用名不能为空"));
        }
        let app = self
            .repo
            .create_app(&NewApplication {
                key: key.to_owned(),
                name: name.trim().to_owned(),
                description: description.trim().to_owned(),
                owner_id: Some(actor.id),
            })
            .await?;
        let _ = self
            .events
            .publish(vec![
                EventEnvelope::new(
                    "app.created",
                    json!({
                        "app_id": app.id.as_uuid(),
                        "key": app.key,
                        "owner": actor.id.as_uuid(),
                    }),
                )
                .with_actor(actor.id.as_uuid().to_string()),
            ])
            .await;
        Ok(app)
    }

    /// 应用列表(登录即可见)。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn list_apps(&self, _actor: &CurrentUser) -> Result<Vec<Application>, AppError> {
        self.repo.list_apps().await
    }

    /// 按 ID 取应用(读权限)。
    ///
    /// # Errors
    /// 应用不存在 `APP-404`;无读权限 `APP-403`。
    pub async fn app_by_id(
        &self,
        actor: &CurrentUser,
        id: ApplicationId,
    ) -> Result<Application, AppError> {
        let app = self
            .repo
            .find_app_by_id(id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "应用不存在"))?;
        self.ensure(actor, Action::Read, id).await?;
        Ok(app)
    }

    /// 按键取应用(读权限;供按 `{key}` 定位的端点复用)。
    ///
    /// # Errors
    /// 应用不存在 `APP-404`;无读权限 `APP-403`。
    pub async fn app_by_key(
        &self,
        actor: &CurrentUser,
        key: &str,
    ) -> Result<Application, AppError> {
        let app = self
            .repo
            .find_app_by_key(key)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "应用不存在"))?;
        self.ensure(actor, Action::Read, app.id).await?;
        Ok(app)
    }

    /// 应用聚合视图(应用 + 环境含计数 + 关联仓库)。
    ///
    /// # Errors
    /// 应用不存在 `APP-404`;无读权限 `APP-403`。
    pub async fn aggregate(
        &self,
        actor: &CurrentUser,
        key: &str,
    ) -> Result<AppAggregate, AppError> {
        let app = self
            .repo
            .find_app_by_key(key)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "应用不存在"))?;
        self.ensure(actor, Action::Read, app.id).await?;

        let envs = self.repo.list_envs(app.id).await?;
        let mut env_briefs = Vec::with_capacity(envs.len());
        for e in envs {
            let targets = self.repo.list_targets(e.id).await?;
            let vars = self.repo.list_vars(e.id).await?;
            env_briefs.push(EnvBrief {
                id: e.id.as_uuid().to_string(),
                name: e.name,
                kind: e.kind.as_str().to_owned(),
                target_count: targets.len(),
                var_count: vars.len(),
                production: e.kind.is_production(),
            });
        }

        let repo_ids = self.repo.list_repo_ids(app.id).await?;
        let named = self.repo.repo_full_names(&repo_ids).await?;
        let repos = named
            .into_iter()
            .map(|(id, full_name)| RepoBrief {
                id: id.to_string(),
                full_name,
            })
            .collect();

        Ok(AppAggregate {
            application: app_brief(&app),
            environments: env_briefs,
            repos,
        })
    }

    /// 建环境。
    ///
    /// # Errors
    /// 应用不存在 `APP-404`;无写权限 `APP-403`;重名 `APP-409`。
    pub async fn create_env(
        &self,
        actor: &CurrentUser,
        app_id: ApplicationId,
        name: &str,
        kind: EnvKind,
    ) -> Result<Environment, AppError> {
        self.app_by_id(actor, app_id).await?;
        self.ensure(actor, Action::Write, app_id).await?;
        if name.trim().is_empty() {
            return Err(AppError::invalid(DOMAIN_CODE, "环境名不能为空"));
        }
        self.repo
            .create_env(&NewEnvironment {
                app_id,
                name: name.trim().to_owned(),
                kind,
            })
            .await
    }

    /// 环境列表。
    ///
    /// # Errors
    /// 应用不存在 `APP-404`;无读权限 `APP-403`。
    pub async fn list_envs(
        &self,
        actor: &CurrentUser,
        app_id: ApplicationId,
    ) -> Result<Vec<Environment>, AppError> {
        self.app_by_id(actor, app_id).await?;
        self.repo.list_envs(app_id).await
    }

    /// 删除环境(级联目标与变量)。
    ///
    /// # Errors
    /// 环境不存在 `APP-404`;无写权限 `APP-403`。
    pub async fn delete_env(
        &self,
        actor: &CurrentUser,
        env_id: EnvironmentId,
    ) -> Result<(), AppError> {
        let env = self.env_or_404(env_id).await?;
        self.ensure(actor, Action::Write, env.app_id).await?;
        if !self.repo.delete_env(env_id).await? {
            return Err(AppError::not_found(DOMAIN_CODE, "环境不存在"));
        }
        Ok(())
    }

    /// 登记部署目标。
    ///
    /// # Errors
    /// 环境不存在 `APP-404`;无写权限 `APP-403`;重名 `APP-409`。
    pub async fn add_target(
        &self,
        actor: &CurrentUser,
        env_id: EnvironmentId,
        name: &str,
        host: &str,
        port: i32,
        labels: Vec<String>,
    ) -> Result<DeployTarget, AppError> {
        let env = self.env_or_404(env_id).await?;
        self.ensure(actor, Action::Write, env.app_id).await?;
        if name.trim().is_empty() || host.trim().is_empty() {
            return Err(AppError::invalid(DOMAIN_CODE, "目标名与主机地址不能为空"));
        }
        if !(1..=65535).contains(&port) {
            return Err(AppError::invalid(DOMAIN_CODE, "端口须在 1~65535"));
        }
        self.repo
            .add_target(&NewDeployTarget {
                env_id,
                name: name.trim().to_owned(),
                host: host.trim().to_owned(),
                port,
                labels,
            })
            .await
    }

    /// 部署目标列表。
    ///
    /// # Errors
    /// 环境不存在 `APP-404`;无读权限 `APP-403`。
    pub async fn list_targets(
        &self,
        actor: &CurrentUser,
        env_id: EnvironmentId,
    ) -> Result<Vec<DeployTarget>, AppError> {
        let env = self.env_or_404(env_id).await?;
        self.ensure(actor, Action::Read, env.app_id).await?;
        self.repo.list_targets(env_id).await
    }

    /// 移除部署目标。
    ///
    /// # Errors
    /// 环境不存在 `APP-404`;无写权限 `APP-403`;目标不存在 `APP-404`。
    pub async fn remove_target(
        &self,
        actor: &CurrentUser,
        env_id: EnvironmentId,
        target_id: DeployTargetId,
    ) -> Result<(), AppError> {
        let env = self.env_or_404(env_id).await?;
        self.ensure(actor, Action::Write, env.app_id).await?;
        if !self.repo.remove_target(target_id).await? {
            return Err(AppError::not_found(DOMAIN_CODE, "部署目标不存在"));
        }
        Ok(())
    }

    /// 写入/覆盖环境变量。
    ///
    /// # Errors
    /// 环境不存在 `APP-404`;无写权限 `APP-403`;键非法 `APP-400`。
    pub async fn upsert_var(
        &self,
        actor: &CurrentUser,
        env_id: EnvironmentId,
        key: &str,
        value: &str,
        is_secret: bool,
    ) -> Result<EnvVar, AppError> {
        let env = self.env_or_404(env_id).await?;
        self.ensure(actor, Action::Write, env.app_id).await?;
        let key = key.trim();
        if key.is_empty()
            || !key
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            return Err(AppError::invalid(
                DOMAIN_CODE,
                "变量名须为字母/数字/下划线/连字符",
            ));
        }
        self.repo
            .upsert_var(&NewEnvVar {
                env_id,
                key: key.to_owned(),
                value: value.to_owned(),
                is_secret,
            })
            .await
    }

    /// 环境变量列表。
    ///
    /// # Errors
    /// 环境不存在 `APP-404`;无读权限 `APP-403`。
    pub async fn list_vars(
        &self,
        actor: &CurrentUser,
        env_id: EnvironmentId,
    ) -> Result<Vec<EnvVar>, AppError> {
        let env = self.env_or_404(env_id).await?;
        self.ensure(actor, Action::Read, env.app_id).await?;
        self.repo.list_vars(env_id).await
    }

    /// 删除环境变量。
    ///
    /// # Errors
    /// 环境不存在 `APP-404`;无写权限 `APP-403`;变量不存在 `APP-404`。
    pub async fn delete_var(
        &self,
        actor: &CurrentUser,
        env_id: EnvironmentId,
        var_id: EnvVarId,
    ) -> Result<(), AppError> {
        let env = self.env_or_404(env_id).await?;
        self.ensure(actor, Action::Write, env.app_id).await?;
        if !self.repo.delete_var(var_id).await? {
            return Err(AppError::not_found(DOMAIN_CODE, "变量不存在"));
        }
        Ok(())
    }

    /// 关联仓库(PAAS-01 聚合)。
    ///
    /// # Errors
    /// 应用不存在 `APP-404`;无写权限 `APP-403`。
    pub async fn link_repo(
        &self,
        actor: &CurrentUser,
        app_id: ApplicationId,
        repo_id: uuid::Uuid,
    ) -> Result<(), AppError> {
        self.app_by_id(actor, app_id).await?;
        self.ensure(actor, Action::Write, app_id).await?;
        self.repo.link_repo(app_id, repo_id).await
    }

    /// 解除仓库关联。
    ///
    /// # Errors
    /// 应用不存在 `APP-404`;无写权限 `APP-403`;未关联 `APP-404`。
    pub async fn unlink_repo(
        &self,
        actor: &CurrentUser,
        app_id: ApplicationId,
        repo_id: uuid::Uuid,
    ) -> Result<(), AppError> {
        self.app_by_id(actor, app_id).await?;
        self.ensure(actor, Action::Write, app_id).await?;
        if !self.repo.unlink_repo(app_id, repo_id).await? {
            return Err(AppError::not_found(DOMAIN_CODE, "该仓库未关联到此应用"));
        }
        Ok(())
    }

    /// 应用规格(规格即代码:提交声明文本 → 解析校验 → 追加新版本)。
    ///
    /// # Errors
    /// 应用/环境不存在 `APP-404`;无写权限 `APP-403`;规格非法 `APP-400`。
    pub async fn apply_spec(
        &self,
        actor: &CurrentUser,
        app_key: &str,
        env_id: EnvironmentId,
        source: &str,
    ) -> Result<SpecVersion, AppError> {
        let app = self.app_by_key(actor, app_key).await?;
        self.ensure(actor, Action::Write, app.id).await?;
        let env = self.env_or_404(env_id).await?;
        if env.app_id != app.id {
            return Err(AppError::invalid(DOMAIN_CODE, "环境不属于该应用"));
        }
        let spec = parse_spec(source)?;
        let (service_id, current) = self.specs.ensure_service(app.id, env_id).await?;
        let version = current + 1;
        self.specs
            .append_spec(service_id, version, &spec, source.trim(), Some(actor.id))
            .await?;
        self.specs.set_current_version(service_id, version).await?;
        let _ = self
            .events
            .publish(vec![
                EventEnvelope::new(
                    "service.spec_applied",
                    json!({
                        "app_id": app.id.as_uuid(),
                        "env_id": env_id.as_uuid(),
                        "version": version,
                        "image": spec.image,
                        "replicas": spec.replicas,
                    }),
                )
                .with_actor(actor.id.as_uuid().to_string()),
            ])
            .await;
        self.specs
            .spec_at(service_id, version)
            .await?
            .ok_or_else(|| AppError::internal(DOMAIN_CODE).with_details(json!("规格写入后读回失败")))
    }

    /// 当前规格(未提交过则为 None)。
    ///
    /// # Errors
    /// 应用不存在 `APP-404`;无读权限 `APP-403`。
    pub async fn current_spec(
        &self,
        actor: &CurrentUser,
        app_key: &str,
        env_id: EnvironmentId,
    ) -> Result<Option<SpecVersion>, AppError> {
        let app = self.app_by_key(actor, app_key).await?;
        let env = self.env_or_404(env_id).await?;
        if env.app_id != app.id {
            return Err(AppError::invalid(DOMAIN_CODE, "环境不属于该应用"));
        }
        let (service_id, _) = self.specs.ensure_service(app.id, env_id).await?;
        self.specs.current_spec(service_id).await
    }

    /// 规格版本历史(新→旧)。
    ///
    /// # Errors
    /// 应用不存在 `APP-404`;无读权限 `APP-403`。
    pub async fn list_spec_versions(
        &self,
        actor: &CurrentUser,
        app_key: &str,
        env_id: EnvironmentId,
    ) -> Result<Vec<SpecVersion>, AppError> {
        let app = self.app_by_key(actor, app_key).await?;
        let env = self.env_or_404(env_id).await?;
        if env.app_id != app.id {
            return Err(AppError::invalid(DOMAIN_CODE, "环境不属于该应用"));
        }
        let (service_id, _) = self.specs.ensure_service(app.id, env_id).await?;
        self.specs.list_spec_versions(service_id).await
    }

    async fn env_or_404(&self, env_id: EnvironmentId) -> Result<Environment, AppError> {
        self.repo
            .find_env(env_id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "环境不存在"))
    }

    async fn ensure(
        &self,
        actor: &CurrentUser,
        action: Action,
        app_id: ApplicationId,
    ) -> Result<(), AppError> {
        let locator = codeplan_kernel::authz::ResourceLocator::App(app_id.as_uuid());
        self.authz
            .check(actor.id, action, &locator.to_ref())
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(json!(e)))?
            .then_some(())
            .ok_or_else(|| AppError::forbidden(DOMAIN_CODE, "无权限执行该操作"))
    }
}

/// 应用 → 摘要。
fn app_brief(a: &Application) -> AppBrief {
    AppBrief {
        id: a.id.as_uuid().to_string(),
        key: a.key.clone(),
        name: a.name.clone(),
        description: a.description.clone(),
        owner: a.owner_id.map(|u| u.as_uuid().to_string()),
        created_at: chrono::DateTime::to_rfc3339(&a.created_at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_key_validation() {
        assert!(valid_app_key("web-app"));
        assert!(valid_app_key("svc1"));
        assert!(!valid_app_key("A"), "太短");
        assert!(!valid_app_key("Web-App"), "不允许大写");
        assert!(!valid_app_key("web_app"), "不允许下划线");
        assert!(!valid_app_key(&"x".repeat(41)), "过长");
    }

    #[test]
    fn env_kind_roundtrip_and_production_flag() {
        for k in [EnvKind::Dev, EnvKind::Test, EnvKind::Staging, EnvKind::Prod] {
            assert_eq!(EnvKind::parse(k.as_str()), Some(k));
        }
        assert!(EnvKind::Prod.is_production());
        assert!(!EnvKind::Staging.is_production());
        assert_eq!(EnvKind::parse("nope"), None);
    }
}

// ---------- 配置与密钥用例(T-317) ----------

impl AppCenterService {
    /// 提交配置(敏感项**加密后**落库;发布 `config.applied`)。
    ///
    /// # Errors
    /// 应用/环境不存在 `APP-404`;无写权限 `APP-403`;声明非法 `APP-400`。
    pub async fn apply_config(
        &self,
        actor: &CurrentUser,
        app_key: &str,
        env_id: EnvironmentId,
        source: &str,
    ) -> Result<ConfigVersion, AppError> {
        let app = self.app_by_key(actor, app_key).await?;
        self.ensure(actor, Action::Write, app.id).await?;
        let env = self.env_or_404(env_id).await?;
        if env.app_id != app.id {
            return Err(AppError::invalid(DOMAIN_CODE, "环境不属于该应用"));
        }
        let mut items = parse_config(source)?;
        // 留痕脱敏:先按明文算出"可安全保存的声明文本",再加密敏感值
        let safe_source = crate::domain::redact_source(source.trim(), &items);
        // 敏感项加密:落库即为密文,明文只存在于本次请求内存中
        let mut secret_count = 0usize;
        for it in &mut items {
            if it.secret {
                it.value = self.key.encrypt(&it.value)?;
                secret_count += 1;
            }
        }
        let (config_id, current) = self.configs.ensure_config(app.id, env_id).await?;
        let version = current + 1;
        self.configs
            .append_config(config_id, version, &items, &safe_source, Some(actor.id))
            .await?;
        self.configs
            .set_current_config_version(config_id, version)
            .await?;
        let _ = self
            .events
            .publish(vec![
                EventEnvelope::new(
                    "config.applied",
                    json!({
                        "app_id": app.id.as_uuid(),
                        "env_id": env_id.as_uuid(),
                        "version": version,
                        "items": items.len(),
                        "secrets": secret_count,
                    }),
                )
                .with_actor(actor.id.as_uuid().to_string()),
            ])
            .await;
        let by = actor.id.as_uuid().to_string();
        Ok(mask_version(
            version,
            &items,
            &safe_source,
            Some(&by),
            &chrono::Utc::now().to_rfc3339(),
        ))
    }

    /// 当前配置(展示形态;敏感项不回显值)。
    ///
    /// # Errors
    /// 应用不存在 `APP-404`;无读权限 `APP-403`。
    pub async fn current_config(
        &self,
        actor: &CurrentUser,
        app_key: &str,
        env_id: EnvironmentId,
    ) -> Result<Option<ConfigVersion>, AppError> {
        let (app, env) = self.app_env(actor, app_key, env_id).await?;
        let _ = env;
        let (config_id, _) = self.configs.ensure_config(app.id, env_id).await?;
        Ok(self
            .configs
            .current_config(config_id)
            .await?
            .map(|(v, items, source, by, at)| mask_version(v, &items, &source, by.as_deref(), &at)))
    }

    /// 配置版本历史(新→旧;敏感项不回显值)。
    ///
    /// # Errors
    /// 应用不存在 `APP-404`;无读权限 `APP-403`。
    pub async fn list_config_versions(
        &self,
        actor: &CurrentUser,
        app_key: &str,
        env_id: EnvironmentId,
    ) -> Result<Vec<ConfigVersion>, AppError> {
        let (app, _env) = self.app_env(actor, app_key, env_id).await?;
        let (config_id, _) = self.configs.ensure_config(app.id, env_id).await?;
        Ok(self
            .configs
            .list_config_versions(config_id)
            .await?
            .into_iter()
            .map(|(v, items, source, by, at)| mask_version(v, &items, &source, by.as_deref(), &at))
            .collect())
    }

    /// 解密单个敏感项(需写权限;发布 `config.secret_revealed` 审计事件)。
    ///
    /// # Errors
    /// 应用不存在 `APP-404`;无写权限 `APP-403`;键不存在 `APP-404`。
    pub async fn reveal_config_key(
        &self,
        actor: &CurrentUser,
        app_key: &str,
        env_id: EnvironmentId,
        key: &str,
    ) -> Result<String, AppError> {
        let (app, _env) = self.app_env(actor, app_key, env_id).await?;
        self.ensure(actor, Action::Write, app.id).await?;
        let (config_id, _) = self.configs.ensure_config(app.id, env_id).await?;
        let Some((_v, items, _s, _by, _at)) = self.configs.current_config(config_id).await? else {
            return Err(AppError::not_found(DOMAIN_CODE, "尚无配置"));
        };
        let item = items
            .iter()
            .find(|i| i.key == key)
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, format!("配置项不存在:{key}")))?;
        let plain = if item.secret {
            self.key.decrypt(&item.value)?
        } else {
            item.value.clone()
        };
        // 审计:解密动作留痕(不记录值本身)
        let _ = self
            .events
            .publish(vec![
                EventEnvelope::new(
                    "config.secret_revealed",
                    json!({
                        "app_id": app.id.as_uuid(),
                        "env_id": env_id.as_uuid(),
                        "key": key,
                        "secret": item.secret,
                    }),
                )
                .with_actor(actor.id.as_uuid().to_string()),
            ])
            .await;
        Ok(plain)
    }

    /// 供**注入**使用的明文键值对(不落库、不经 HTTP 暴露;由部署执行链路调用)。
    ///
    /// # Errors
    /// 应用不存在 `APP-404`;无读权限 `APP-403`;解密失败。
    pub async fn resolved_config_items(
        &self,
        actor: &CurrentUser,
        app_key: &str,
        env_id: EnvironmentId,
    ) -> Result<Vec<(String, String)>, AppError> {
        let (app, _env) = self.app_env(actor, app_key, env_id).await?;
        let (config_id, _) = self.configs.ensure_config(app.id, env_id).await?;
        let Some((_v, items, _s, _by, _at)) = self.configs.current_config(config_id).await? else {
            return Ok(Vec::new());
        };
        let mut out = Vec::with_capacity(items.len());
        for i in items {
            let value = if i.secret {
                self.key.decrypt(&i.value)?
            } else {
                i.value
            };
            out.push((i.key, value));
        }
        Ok(out)
    }

    /// 解析应用 + 环境并校验归属。
    async fn app_env(
        &self,
        actor: &CurrentUser,
        app_key: &str,
        env_id: EnvironmentId,
    ) -> Result<(Application, Environment), AppError> {
        let app = self.app_by_key(actor, app_key).await?;
        let env = self.env_or_404(env_id).await?;
        if env.app_id != app.id {
            return Err(AppError::invalid(DOMAIN_CODE, "环境不属于该应用"));
        }
        Ok((app, env))
    }
}

/// 存储形态 → 展示形态(敏感项值置空)。
fn mask_version(
    version: i32,
    items: &[ConfigItem],
    source: &str,
    created_by: Option<&str>,
    created_at: &str,
) -> ConfigVersion {
    ConfigVersion {
        version,
        items: items
            .iter()
            .map(|i| ConfigItemView {
                key: i.key.clone(),
                value: if i.secret { None } else { Some(i.value.clone()) },
                secret: i.secret,
            })
            .collect(),
        source: source.to_owned(),
        created_by: created_by.map(str::to_owned),
        created_at: created_at.to_owned(),
    }
}

// ---------- 域名绑定与网关路由(T-315) ----------

impl AppCenterService {
    /// 绑定自定义域名(主机名全局唯一;发布 `domain.bound`)。
    ///
    /// # Errors
    /// 应用/环境不存在 `APP-404`;无写权限 `APP-403`;主机名非法 `APP-400`;已被占用 `APP-409`。
    pub async fn bind_domain(
        &self,
        actor: &CurrentUser,
        app_key: &str,
        env_id: EnvironmentId,
        host: &str,
        tls_enabled: bool,
    ) -> Result<crate::domain::AppDomain, AppError> {
        let (app, _env) = self.app_env(actor, app_key, env_id).await?;
        self.ensure(actor, Action::Write, app.id).await?;
        let host = host.trim().to_ascii_lowercase();
        if !crate::gateway::valid_hostname(&host) {
            return Err(AppError::invalid(
                DOMAIN_CODE,
                "主机名非法(须为多段域名,标签仅字母/数字/连字符)",
            ));
        }
        if let Some(existing) = self.domains.find_by_host(&host).await? {
            return Err(AppError::conflict(
                DOMAIN_CODE,
                format!("域名已被环境占用:{}", existing.env_id.as_uuid()),
            ));
        }
        let d = self
            .domains
            .bind(&NewDomain {
                env_id,
                host: host.clone(),
                kind: DomainKind::Custom,
                tls_enabled,
            })
            .await?;
        self.publish_domain_event("domain.bound", actor, app.id, env_id, &host).await;
        Ok(d)
    }

    /// 自动分配域名(幂等:已分配则直接返回)。
    ///
    /// # Errors
    /// 应用/环境不存在 `APP-404`;无写权限 `APP-403`。
    pub async fn auto_assign_domain(
        &self,
        actor: &CurrentUser,
        app_key: &str,
        env_id: EnvironmentId,
    ) -> Result<crate::domain::AppDomain, AppError> {
        let (app, env) = self.app_env(actor, app_key, env_id).await?;
        self.ensure(actor, Action::Write, app.id).await?;
        let existing = self.domains.list_by_env(env_id).await?;
        if let Some(d) = existing.iter().find(|d| d.kind == DomainKind::Auto) {
            return Ok(d.clone());
        }
        let host = crate::gateway::auto_host(&app.key, &env.name, &crate::gateway::domain_suffix());
        // 极端情况下自动域名也可能撞车(同名应用同名环境已被占用)
        if self.domains.find_by_host(&host).await?.is_some() {
            return Err(AppError::conflict(
                DOMAIN_CODE,
                format!("自动域名已被占用:{host},请改用自定义域名"),
            ));
        }
        let d = self
            .domains
            .bind(&NewDomain {
                env_id,
                host: host.clone(),
                kind: DomainKind::Auto,
                tls_enabled: true,
            })
            .await?;
        self.publish_domain_event("domain.bound", actor, app.id, env_id, &host).await;
        Ok(d)
    }

    /// 环境的域名列表。
    ///
    /// # Errors
    /// 应用不存在 `APP-404`;无读权限 `APP-403`。
    pub async fn list_domains(
        &self,
        actor: &CurrentUser,
        app_key: &str,
        env_id: EnvironmentId,
    ) -> Result<Vec<crate::domain::AppDomain>, AppError> {
        let (_app, _env) = self.app_env(actor, app_key, env_id).await?;
        self.domains.list_by_env(env_id).await
    }

    /// 解绑域名。
    ///
    /// # Errors
    /// 应用不存在 `APP-404`;无写权限 `APP-403`;绑定不存在 `APP-404`。
    pub async fn unbind_domain(
        &self,
        actor: &CurrentUser,
        app_key: &str,
        env_id: EnvironmentId,
        domain_id: DomainId,
    ) -> Result<(), AppError> {
        let (app, _env) = self.app_env(actor, app_key, env_id).await?;
        self.ensure(actor, Action::Write, app.id).await?;
        if !self.domains.unbind(domain_id).await? {
            return Err(AppError::not_found(DOMAIN_CODE, "域名绑定不存在"));
        }
        Ok(())
    }

    /// 组装网关路由(域名 + 部署目标 + 规格端口)。
    ///
    /// # Errors
    /// 应用不存在 `APP-404`;无读权限 `APP-403`。
    pub async fn gateway_route(
        &self,
        actor: &CurrentUser,
        app_key: &str,
        env_id: EnvironmentId,
    ) -> Result<crate::gateway::GatewayRoute, AppError> {
        let (app, env) = self.app_env(actor, app_key, env_id).await?;
        let domains = self.domains.list_by_env(env_id).await?;
        let targets = self.repo.list_targets(env_id).await?;
        // 上游端口取当前规格的首个端口(缺省 8080)
        let (service_id, _) = self.specs.ensure_service(app.id, env_id).await?;
        let upstream_port = self
            .specs
            .current_spec(service_id)
            .await?
            .and_then(|s| s.spec.ports.first().map(|p| p.container))
            .unwrap_or(8080);
        let tls_enabled = domains.iter().any(|d| d.tls_enabled);
        let cert_ref = domains
            .iter()
            .find(|d| !d.cert_ref.is_empty())
            .map(|d| d.cert_ref.clone())
            .unwrap_or_default();
        Ok(crate::gateway::GatewayRoute {
            app_key: app.key,
            env_name: env.name,
            hosts: domains.into_iter().map(|d| d.host).collect(),
            // 目标端口入库时已校验 1~65535,这里做一次安全转换
            upstreams: targets
                .into_iter()
                .map(|t| (t.host, u16::try_from(t.port).unwrap_or(22)))
                .collect(),
            upstream_port,
            tls_enabled,
            cert_ref,
        })
    }

    /// 渲染 Nginx 站点配置。
    ///
    /// # Errors
    /// 同 [`Self::gateway_route`]。
    pub async fn gateway_nginx(
        &self,
        actor: &CurrentUser,
        app_key: &str,
        env_id: EnvironmentId,
    ) -> Result<String, AppError> {
        let route = self.gateway_route(actor, app_key, env_id).await?;
        Ok(crate::gateway::render_nginx(&route))
    }

    /// 渲染 APISIX 路由(含建议的路由 ID)。
    ///
    /// # Errors
    /// 同 [`Self::gateway_route`]。
    pub async fn gateway_apisix(
        &self,
        actor: &CurrentUser,
        app_key: &str,
        env_id: EnvironmentId,
    ) -> Result<(String, serde_json::Value), AppError> {
        let route = self.gateway_route(actor, app_key, env_id).await?;
        let id = format!("codeplan-{}-{}", route.app_key, route.env_name);
        Ok((id.clone(), crate::gateway::render_apisix(&route, &id)))
    }

    /// 落地 Nginx 配置到 `CODEPLAN_GATEWAY_CONF_DIR`(部署引擎据此 reload)。
    ///
    /// 未配置目录时返回明确错误,而不是静默假装成功。
    ///
    /// # Errors
    /// 同 [`Self::gateway_route`];未配置配置目录或写入失败。
    pub async fn apply_gateway(
        &self,
        actor: &CurrentUser,
        app_key: &str,
        env_id: EnvironmentId,
    ) -> Result<(String, usize), AppError> {
        let route = self.gateway_route(actor, app_key, env_id).await?;
        let conf_dir = std::env::var("CODEPLAN_GATEWAY_CONF_DIR").unwrap_or_default();
        if conf_dir.trim().is_empty() {
            return Err(AppError::internal(DOMAIN_CODE).with_details(json!(
                "未配置 CODEPLAN_GATEWAY_CONF_DIR;可先取 /gateway/nginx 内容手工落地"
            )));
        }
        let cfg = crate::gateway::render_nginx(&route);
        let path = std::path::PathBuf::from(conf_dir.trim()).join(format!(
            "{}-{}.conf",
            route.app_key, route.env_name
        ));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::internal(DOMAIN_CODE).with_details(json!(format!("创建配置目录失败:{e}")))
            })?;
        }
        std::fs::write(&path, &cfg).map_err(|e| {
            AppError::internal(DOMAIN_CODE).with_details(json!(format!("写入配置失败:{e}")))
        })?;
        Ok((path.to_string_lossy().into_owned(), cfg.len()))
    }

    async fn publish_domain_event(
        &self,
        event: &str,
        actor: &CurrentUser,
        app_id: ApplicationId,
        env_id: EnvironmentId,
        host: &str,
    ) {
        let _ = self
            .events
            .publish(vec![
                EventEnvelope::new(
                    event,
                    json!({
                        "app_id": app_id.as_uuid(),
                        "env_id": env_id.as_uuid(),
                        "host": host,
                    }),
                )
                .with_actor(actor.id.as_uuid().to_string()),
            ])
            .await;
    }
}

// ---------- 证书签发与续期(T-316 / PAAS-07) ----------

/// 证书文件目录(网关按 `/etc/codeplan/certs` 挂载该目录)。
#[must_use]
pub fn cert_dir() -> std::path::PathBuf {
    std::env::var("CODEPLAN_GATEWAY_CERT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("./data/certs"))
}

/// 证书引用:网关按 `{cert_ref}.crt/.key` 定位文件。
#[must_use]
pub fn cert_ref_for(app_key: &str, env_name: &str) -> String {
    let clean = |s: &str| {
        s.to_ascii_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .trim_matches('-')
            .to_owned()
    };
    format!("{}-{}", clean(app_key), clean(env_name))
}

impl AppCenterService {
    /// 申请/续期证书:签发 → 落盘(部署钩子)→ 记录 → 回填域名 `cert_ref`。
    ///
    /// # Errors
    /// 应用/环境不存在 `APP-404`;无写权限 `APP-403`;未绑定域名或未装配签发器 `APP-400`;签发失败。
    pub async fn issue_certificate(
        &self,
        actor: &CurrentUser,
        app_key: &str,
        env_id: EnvironmentId,
    ) -> Result<Certificate, AppError> {
        let (app, env) = self.app_env(actor, app_key, env_id).await?;
        self.ensure(actor, Action::Write, app.id).await?;
        let Some(issuer) = self.issuer.clone() else {
            return Err(AppError::invalid(
                DOMAIN_CODE,
                "未装配证书签发器(需配置 CODEPLAN_ACME_DIRECTORY 启用 cert-acme 插件)",
            ));
        };
        let domains: Vec<String> = self
            .domains
            .list_by_env(env_id)
            .await?
            .into_iter()
            .filter(|d| d.tls_enabled)
            .map(|d| d.host)
            .collect();
        if domains.is_empty() {
            return Err(AppError::invalid(
                DOMAIN_CODE,
                "该环境没有启用 TLS 的域名,请先绑定域名",
            ));
        }
        let issued = issuer.issue(&domains).await.map_err(|e| {
            AppError::internal(DOMAIN_CODE).with_details(json!(format!("证书签发失败:{e}")))
        })?;

        // 部署钩子:证书落盘到网关读取的目录
        let cert_ref = cert_ref_for(&app.key, &env.name);
        let dir = cert_dir();
        std::fs::create_dir_all(&dir).map_err(|e| {
            AppError::internal(DOMAIN_CODE).with_details(json!(format!("创建证书目录失败:{e}")))
        })?;
        std::fs::write(dir.join(format!("{cert_ref}.crt")), &issued.cert_pem).map_err(|e| {
            AppError::internal(DOMAIN_CODE).with_details(json!(format!("写入证书失败:{e}")))
        })?;
        std::fs::write(dir.join(format!("{cert_ref}.key")), &issued.key_pem).map_err(|e| {
            AppError::internal(DOMAIN_CODE).with_details(json!(format!("写入私钥失败:{e}")))
        })?;

        let not_after = issued
            .not_after
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.with_timezone(&chrono::Utc));
        let cert = self
            .certs
            .upsert_cert(
                env_id,
                &cert_ref,
                &domains,
                issuer.name(),
                "issued",
                not_after,
            )
            .await?;
        // 回填域名引用:网关渲染据此启用 TLS 分支(T-315)
        for d in self.domains.list_by_env(env_id).await? {
            if d.tls_enabled && d.cert_ref != cert_ref {
                self.domains.set_cert_ref(d.id, &cert_ref).await?;
            }
        }
        let _ = self
            .events
            .publish(vec![
                EventEnvelope::new(
                    "cert.issued",
                    json!({
                        "app_id": app.id.as_uuid(),
                        "env_id": env_id.as_uuid(),
                        "cert_ref": cert_ref,
                        "domains": domains,
                        "provider": issuer.name(),
                    }),
                )
                .with_actor(actor.id.as_uuid().to_string()),
            ])
            .await;
        Ok(cert)
    }

    /// 环境的证书列表。
    ///
    /// # Errors
    /// 应用不存在 `APP-404`;无读权限 `APP-403`。
    pub async fn list_certificates(
        &self,
        actor: &CurrentUser,
        app_key: &str,
        env_id: EnvironmentId,
    ) -> Result<Vec<Certificate>, AppError> {
        let (_app, _env) = self.app_env(actor, app_key, env_id).await?;
        self.certs.list_certs(env_id).await
    }

    /// 续期扫描:对 `within_days` 天内到期的证书重新签发(供定时任务调用)。
    ///
    /// 以系统身份执行(无 actor),失败仅记录日志、不中断其他证书。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn renew_due_certificates(&self, within_days: i64) -> Result<usize, AppError> {
        let Some(issuer) = self.issuer.clone() else {
            return Ok(0);
        };
        let cutoff = chrono::Utc::now() + chrono::Duration::days(within_days);
        let due = self.certs.expiring_before(cutoff).await?;
        let mut renewed = 0usize;
        for cert in due {
            let domains = cert.domains.clone();
            match issuer.issue(&domains).await {
                Ok(issued) => {
                    let dir = cert_dir();
                    if std::fs::create_dir_all(&dir).is_ok()
                        && std::fs::write(
                            dir.join(format!("{}.crt", cert.cert_ref)),
                            &issued.cert_pem,
                        )
                        .is_ok()
                        && std::fs::write(
                            dir.join(format!("{}.key", cert.cert_ref)),
                            &issued.key_pem,
                        )
                        .is_ok()
                    {
                        let not_after = issued
                            .not_after
                            .as_deref()
                            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                            .map(|d| d.with_timezone(&chrono::Utc));
                        let _ = self
                            .certs
                            .upsert_cert(
                                cert.env_id,
                                &cert.cert_ref,
                                &domains,
                                issuer.name(),
                                "issued",
                                not_after,
                            )
                            .await;
                        renewed += 1;
                        println!("[cert] 已续期 {}", cert.cert_ref);
                    }
                }
                Err(e) => eprintln!("[cert] 续期失败 {}:{e}", cert.cert_ref),
            }
        }
        Ok(renewed)
    }
}

// ---------- 部署主机与 agent 注册(T-311 / PAAS-03) ----------

/// 生成一次性注册令牌(32 字节随机 → base64url)。
#[must_use]
pub fn new_host_token() -> String {
    use base64::Engine as _;
    let bytes: [u8; 32] = rand::random();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// 令牌哈希(库中只存哈希,明文仅下发一次)。
#[must_use]
pub fn hash_host_token(token: &str) -> String {
    use base64::Engine as _;
    use sha2::{Digest as _, Sha256};
    let mut h = Sha256::new();
    h.update(token.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(h.finalize())
}

impl AppCenterService {
    /// 登记部署主机(平台管理员):返回主机与**仅此一次**可见的注册令牌。
    ///
    /// # Errors
    /// 无权限 `APP-403`;名称重复 `APP-409`。
    pub async fn create_host(
        &self,
        actor: &CurrentUser,
        name: &str,
        labels: Vec<String>,
    ) -> Result<(DeployHost, String), AppError> {
        if !actor.is_platform_admin {
            return Err(AppError::forbidden(DOMAIN_CODE, "仅平台管理员可登记部署主机"));
        }
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::invalid(DOMAIN_CODE, "主机名不能为空"));
        }
        let token = new_host_token();
        let host = self
            .hosts
            .create_host(name, &hash_host_token(&token), &labels)
            .await?;
        Ok((host, token))
    }

    /// 主机列表(平台管理员)。
    ///
    /// # Errors
    /// 无权限 `APP-403`。
    pub async fn list_hosts(&self, actor: &CurrentUser) -> Result<Vec<DeployHost>, AppError> {
        if !actor.is_platform_admin {
            return Err(AppError::forbidden(DOMAIN_CODE, "仅平台管理员可查看部署主机"));
        }
        self.hosts.list_hosts().await
    }

    /// agent 注册(无 actor:令牌即凭据)。
    ///
    /// 令牌**一次性**:成功后立即失效(库中标记 registered,同令牌再注册会被拒)。
    ///
    /// # Errors
    /// 令牌无效 `APP-403`;令牌已使用 `APP-409`。
    pub async fn register_agent(
        &self,
        token: &str,
        hostname: &str,
        agent_version: &str,
        labels: Vec<String>,
    ) -> Result<DeployHost, AppError> {
        let token = token.trim();
        if token.is_empty() {
            return Err(AppError::forbidden(DOMAIN_CODE, "缺少注册令牌"));
        }
        let host = self
            .hosts
            .find_by_token_hash(&hash_host_token(token))
            .await?
            .ok_or_else(|| AppError::forbidden(DOMAIN_CODE, "注册令牌无效"))?;
        if host.registered {
            return Err(AppError::conflict(
                DOMAIN_CODE,
                "该注册令牌已被使用,请在平台重新登记主机",
            ));
        }
        self.hosts
            .mark_registered(host.id, hostname, agent_version, &labels)
            .await?;
        let _ = self
            .events
            .publish(vec![
                EventEnvelope::new(
                    "host.registered",
                    json!({
                        "host_id": host.id.as_uuid(),
                        "name": host.name,
                        "hostname": hostname,
                        "agent_version": agent_version,
                    }),
                )
                .with_actor("agent".to_owned()),
            ])
            .await;
        self.hosts
            .find_host(host.id)
            .await?
            .ok_or_else(|| AppError::internal(DOMAIN_CODE).with_details(json!("注册后读回失败")))
    }

    /// agent 心跳(无 actor)。
    ///
    /// # Errors
    /// 主机不存在 `APP-404`。
    pub async fn heartbeat_agent(&self, host_id: DeployHostId) -> Result<(), AppError> {
        if self.hosts.find_host(host_id).await?.is_none() {
            return Err(AppError::not_found(DOMAIN_CODE, "主机不存在"));
        }
        self.hosts.touch_heartbeat(host_id).await
    }
}

// ---------- 部署执行(T-313 / PAAS-04) ----------

/// 容器名:`codeplan-{app}-{env}-{序号}`(稳定命名,重建时按名收敛)。
#[must_use]
pub fn container_name(app_key: &str, env_name: &str, index: u32) -> String {
    let clean = |s: &str| {
        s.to_ascii_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .trim_matches('-')
            .to_owned()
    };
    format!("codeplan-{}-{}-{}", clean(app_key), clean(env_name), index + 1)
}

/// 解析规格中的 `${KEY}` 引用(从配置项取值;未定义的原样保留并计入告警)。
#[must_use]
pub fn resolve_env_refs(
    bindings: &[crate::domain::EnvBinding],
    config: &[(String, String)],
) -> (Vec<(String, String)>, Vec<String>) {
    let map: std::collections::HashMap<&str, &str> = config
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let mut out = Vec::with_capacity(bindings.len());
    let mut missing = Vec::new();
    for b in bindings {
        let mut value = b.value.clone();
        // 形如 ${KEY} 的值整体替换;找不到时保留原文并记录
        if let Some(key) = value
            .strip_prefix("${")
            .and_then(|r| r.strip_suffix('}'))
        {
            match map.get(key) {
                Some(v) => value = (*v).to_owned(),
                None => missing.push(key.to_owned()),
            }
        }
        out.push((b.name.clone(), value));
    }
    (out, missing)
}

impl AppCenterService {
    /// 一键部署:按当前规格生成期望状态,下发到在线主机(发布 `deployment.created`)。
    ///
    /// # Errors
    /// 应用/环境不存在 `APP-404`;无写权限 `APP-403`;无规格/无在线主机/未装配投递口 `APP-400`。
    pub async fn deploy(
        &self,
        actor: &CurrentUser,
        app_key: &str,
        env_id: EnvironmentId,
    ) -> Result<Deployment, AppError> {
        let (app, env) = self.app_env(actor, app_key, env_id).await?;
        self.ensure(actor, Action::Write, app.id).await?;
        let Some(sink) = self.agent.clone() else {
            return Err(AppError::invalid(DOMAIN_CODE, "未装配 agent 投递通道,无法下发部署"));
        };
        // 期望状态来源:当前规格(必需)+ 当前配置(用于注入)
        let (service_id, _) = self.specs.ensure_service(app.id, env_id).await?;
        let Some(spec) = self.specs.current_spec(service_id).await? else {
            return Err(AppError::invalid(DOMAIN_CODE, "该环境尚未提交服务规格,无法部署"));
        };
        let config = self.resolved_config_items(actor, app_key, env_id).await?;
        let (env_pairs, missing) = resolve_env_refs(&spec.spec.env, &config);
        if !missing.is_empty() {
            return Err(AppError::invalid(
                DOMAIN_CODE,
                format!("规格引用的配置项未定义:{}", missing.join(",")),
            ));
        }

        // 目标主机:已注册且**心跳新鲜**(按标签选择留待后续)
        let now = chrono::Utc::now();
        let hosts: Vec<DeployHost> = self
            .hosts
            .list_hosts()
            .await?
            .into_iter()
            .filter(|h| {
                h.registered
                    && crate::domain::is_fresh_for_deploy(h.status, h.last_heartbeat_at, now)
            })
            .collect();
        if hosts.is_empty() {
            return Err(AppError::invalid(
                DOMAIN_CODE,
                "没有心跳正常的主机,请确认目标主机上的 agent 在运行",
            ));
        }
        let target_ids: Vec<DeployHostId> = hosts.iter().map(|h| h.id).collect();

        // 容器计划:副本数 × 单容器模板
        let env_map: std::collections::BTreeMap<String, String> = env_pairs.into_iter().collect();
        let containers: Vec<serde_json::Value> = (0..spec.spec.replicas)
            .map(|i| {
                serde_json::json!({
                    "name": container_name(&app.key, &env.name, i),
                    "image": spec.spec.image,
                    "ports": spec.spec.ports,
                    "env": env_map,
                    "volumes": spec.spec.volumes,
                    "healthcheck": spec.spec.healthcheck,
                })
            })
            .collect();
        let plan = serde_json::json!({
            "deployment_id": "",
            "app": app.key,
            "env": env.name,
            "containers": containers,
        });

        let detail = format!("规格 v{} · {} 个副本 · {} 台主机", spec.version, spec.spec.replicas, hosts.len());
        let mut record = self
            .deployments
            .create_deployment(env_id, &app.key, Some(spec.version), &target_ids, &detail)
            .await?;
        // 计划里回填真实部署 ID(便于 agent 回执对齐)
        let mut plan = plan;
        if let Some(obj) = plan.as_object_mut() {
            obj.insert(
                "deployment_id".to_owned(),
                serde_json::json!(record.id.as_uuid().to_string()),
            );
        }
        let payload = serde_json::to_string(&plan).map_err(|e| {
            AppError::internal(DOMAIN_CODE).with_details(json!(format!("计划序列化失败:{e}")))
        })?;

        let mut pushed = 0usize;
        for h in &hosts {
            match sink.push(h.id, "deploy", &payload).await {
                Ok(Some(command_id)) => {
                    if let Ok(uuid) = uuid::Uuid::parse_str(&command_id) {
                        self.deployments
                            .record_command(uuid, record.id, h.id, "deploy")
                            .await?;
                        pushed += 1;
                    }
                }
                Ok(None) => eprintln!("[deploy] 主机 {} 离线,跳过", h.name),
                Err(e) => eprintln!("[deploy] 主机 {} 下发失败:{e}", h.name),
            }
        }
        if pushed == 0 {
            let _ = self
                .deployments
                .record_result(record.id, target_ids[0], "ERR 无主机接受部署命令")
                .await;
        }
        let _ = self
            .events
            .publish(vec![
                EventEnvelope::new(
                    "deployment.created",
                    json!({
                        "deployment_id": record.id.as_uuid(),
                        "app_id": app.id.as_uuid(),
                        "env_id": env_id.as_uuid(),
                        "spec_version": spec.version,
                        "hosts": target_ids.iter().map(|h| h.as_uuid()).collect::<Vec<_>>(),
                        "pushed": pushed,
                    }),
                )
                .with_actor(actor.id.as_uuid().to_string()),
            ])
            .await;
        record = self
            .deployments
            .find_deployment(record.id)
            .await?
            .unwrap_or(record);
        Ok(record)
    }

    /// agent 回执(无 actor):按命令 ID 路由回部署记录并刷新状态。
    ///
    /// # Errors
    /// 命令不存在 `APP-404`。
    pub async fn record_agent_result(
        &self,
        command_id: &str,
        ok: bool,
        message: &str,
    ) -> Result<Option<Deployment>, AppError> {
        let Ok(uuid) = uuid::Uuid::parse_str(command_id) else {
            return Ok(None);
        };
        let Some((deployment, host_id)) = self.deployments.deployment_by_command(uuid).await? else {
            return Ok(None);
        };
        let result = format!("{} {message}", if ok { "OK" } else { "ERR" });
        Ok(Some(
            self.deployments
                .record_result(deployment.id, host_id, &result)
                .await?,
        ))
    }

    /// 收敛超时部署(供组合根定时调用);返回本轮收敛条数。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn expire_stale_deployments(&self) -> Result<usize, AppError> {
        let before = chrono::Utc::now() - chrono::Duration::minutes(crate::domain::DEPLOY_TIMEOUT_MINUTES);
        self.deployments.expire_pending(before).await
    }

    /// 环境的部署记录。
    ///
    /// # Errors
    /// 应用不存在 `APP-404`;无读权限 `APP-403`。
    pub async fn list_deployments(
        &self,
        actor: &CurrentUser,
        app_key: &str,
        env_id: EnvironmentId,
        limit: i64,
    ) -> Result<Vec<Deployment>, AppError> {
        let (_app, _env) = self.app_env(actor, app_key, env_id).await?;
        self.deployments
            .list_deployments(env_id, limit.clamp(1, 100))
            .await
    }
}

#[cfg(test)]
mod deploy_tests {
    use super::*;
    use crate::domain::EnvBinding;

    #[test]
    fn container_name_is_stable_and_sanitized() {
        assert_eq!(container_name("web-portal", "dev", 0), "codeplan-web-portal-dev-1");
        assert_eq!(container_name("web-portal", "dev", 2), "codeplan-web-portal-dev-3");
        assert_eq!(container_name("a_b", "s t", 0), "codeplan-a-b-s-t-1");
    }

    #[test]
    fn resolves_config_references() {
        let bindings = vec![
            EnvBinding { name: "LOG_LEVEL".to_owned(), value: "${LOG_LEVEL}".to_owned() },
            EnvBinding { name: "STATIC".to_owned(), value: "fixed".to_owned() },
        ];
        let config = vec![("LOG_LEVEL".to_owned(), "debug".to_owned())];
        let (out, missing) = resolve_env_refs(&bindings, &config);
        assert!(missing.is_empty());
        assert_eq!(out[0], ("LOG_LEVEL".to_owned(), "debug".to_owned()));
        assert_eq!(out[1], ("STATIC".to_owned(), "fixed".to_owned()), "非引用值原样保留");
    }

    #[test]
    fn reports_missing_config_reference() {
        let bindings = vec![EnvBinding { name: "X".to_owned(), value: "${NOPE}".to_owned() }];
        let (out, missing) = resolve_env_refs(&bindings, &[]);
        assert_eq!(missing, vec!["NOPE".to_owned()]);
        assert_eq!(out[0].1, "${NOPE}", "未定义时保留原文,由调用方决定报错");
    }
}
