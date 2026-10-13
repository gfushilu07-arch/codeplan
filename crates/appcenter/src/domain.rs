//! 应用中心领域模型与仓储 trait(纯领域,不依赖 axum/sqlx)。

use chrono::{DateTime, Utc};
use codeplan_common::define_id;
use codeplan_common::error::AppError;
use codeplan_kernel::ids::UserId;
use serde::{Deserialize, Serialize};

define_id!(
    /// 应用 ID。
    ApplicationId
);
define_id!(
    /// 环境 ID。
    EnvironmentId
);
define_id!(
    /// 部署目标 ID。
    DeployTargetId
);
define_id!(
    /// 环境变量 ID。
    EnvVarId
);
define_id!(
    /// 服务 ID(应用 × 环境 唯一)。
    ServiceId
);

/// 环境类型(PAAS-02:开发/测试/预发/生产)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvKind {
    /// 开发。
    Dev,
    /// 测试。
    Test,
    /// 预发。
    Staging,
    /// 生产。
    Prod,
}

impl EnvKind {
    /// 存储值。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Test => "test",
            Self::Staging => "staging",
            Self::Prod => "prod",
        }
    }

    /// 解析。
    #[must_use]
    pub fn parse(v: &str) -> Option<Self> {
        match v {
            "dev" => Some(Self::Dev),
            "test" => Some(Self::Test),
            "staging" => Some(Self::Staging),
            "prod" => Some(Self::Prod),
            _ => None,
        }
    }

    /// 是否生产环境(发布管控按此加严,PAAS-14)。
    #[must_use]
    pub fn is_production(self) -> bool {
        matches!(self, Self::Prod)
    }
}

/// 应用(一等实体,聚合仓库/流水线/环境/发布记录)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Application {
    /// 应用 ID。
    pub id: ApplicationId,
    /// 应用键(唯一)。
    pub key: String,
    /// 名称。
    pub name: String,
    /// 描述。
    pub description: String,
    /// 负责人。
    pub owner_id: Option<UserId>,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 更新时间。
    pub updated_at: DateTime<Utc>,
}

/// 创建应用入参。
#[derive(Debug, Clone)]
pub struct NewApplication {
    /// 应用键(小写字母/数字/连字符)。
    pub key: String,
    /// 名称。
    pub name: String,
    /// 描述。
    pub description: String,
    /// 负责人。
    pub owner_id: Option<UserId>,
}

/// 环境。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    /// 环境 ID。
    pub id: EnvironmentId,
    /// 所属应用。
    pub app_id: ApplicationId,
    /// 环境名(应用内唯一)。
    pub name: String,
    /// 环境类型。
    pub kind: EnvKind,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 新建环境入参。
#[derive(Debug, Clone)]
pub struct NewEnvironment {
    /// 所属应用。
    pub app_id: ApplicationId,
    /// 环境名。
    pub name: String,
    /// 环境类型。
    pub kind: EnvKind,
}

/// 部署目标(主机登记,PAAS-02)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployTarget {
    /// 目标 ID。
    pub id: DeployTargetId,
    /// 所属环境。
    pub env_id: EnvironmentId,
    /// 目标名(环境内唯一)。
    pub name: String,
    /// 主机地址。
    pub host: String,
    /// SSH/管理端口。
    pub port: i32,
    /// 标签(用于调度与选择)。
    pub labels: Vec<String>,
    /// 登记时间。
    pub created_at: DateTime<Utc>,
}

/// 新建部署目标入参。
#[derive(Debug, Clone)]
pub struct NewDeployTarget {
    /// 所属环境。
    pub env_id: EnvironmentId,
    /// 目标名。
    pub name: String,
    /// 主机地址。
    pub host: String,
    /// 端口。
    pub port: i32,
    /// 标签。
    pub labels: Vec<String>,
}

/// 环境变量。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvVar {
    /// 变量 ID。
    pub id: EnvVarId,
    /// 所属环境。
    pub env_id: EnvironmentId,
    /// 键。
    pub key: String,
    /// 值(敏感项在 API 层脱敏)。
    pub value: String,
    /// 是否敏感。
    pub is_secret: bool,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 新建/覆盖环境变量入参。
#[derive(Debug, Clone)]
pub struct NewEnvVar {
    /// 所属环境。
    pub env_id: EnvironmentId,
    /// 键。
    pub key: String,
    /// 值。
    pub value: String,
    /// 是否敏感。
    pub is_secret: bool,
}

/// 应用聚合视图(PAAS-01)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct AppAggregate {
    /// 应用。
    pub application: AppBrief,
    /// 环境(含目标数与变量数)。
    pub environments: Vec<EnvBrief>,
    /// 关联仓库(只读投影的展示名)。
    pub repos: Vec<RepoBrief>,
}

/// 应用摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct AppBrief {
    /// ID。
    pub id: String,
    /// 键。
    pub key: String,
    /// 名称。
    pub name: String,
    /// 描述。
    pub description: String,
    /// 负责人。
    pub owner: Option<String>,
    /// 创建时间。
    pub created_at: String,
}

/// 环境摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct EnvBrief {
    /// ID。
    pub id: String,
    /// 名称。
    pub name: String,
    /// 类型。
    pub kind: String,
    /// 部署目标数。
    pub target_count: usize,
    /// 变量数。
    pub var_count: usize,
    /// 是否生产。
    pub production: bool,
}

/// 关联仓库摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct RepoBrief {
    /// 仓库 ID。
    pub id: String,
    /// 全名。
    pub full_name: String,
}

/// 应用键规则:小写字母/数字/连字符,2~40 位。
#[must_use]
pub fn valid_app_key(key: &str) -> bool {
    let b = key.as_bytes();
    (2..=40).contains(&b.len())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// 应用中心仓储。
#[async_trait::async_trait]
pub trait AppCenterRepository: Send + Sync {
    /// 建应用(键冲突返回 `APP-409`)。
    ///
    /// # Errors
    /// 存储故障或唯一键冲突。
    async fn create_app(&self, app: &NewApplication) -> Result<Application, AppError>;

    /// 应用列表(创建时间倒序)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_apps(&self) -> Result<Vec<Application>, AppError>;

    /// 按键取应用。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_app_by_key(&self, key: &str) -> Result<Option<Application>, AppError>;

    /// 按 ID 取应用。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_app_by_id(&self, id: ApplicationId) -> Result<Option<Application>, AppError>;

    /// 建环境(应用内重名返回 `APP-409`)。
    ///
    /// # Errors
    /// 存储故障或唯一键冲突。
    async fn create_env(&self, env: &NewEnvironment) -> Result<Environment, AppError>;

    /// 应用的环境列表。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_envs(&self, app_id: ApplicationId) -> Result<Vec<Environment>, AppError>;

    /// 按 ID 取环境。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_env(&self, id: EnvironmentId) -> Result<Option<Environment>, AppError>;

    /// 删除环境(级联删除目标与变量)。
    ///
    /// # Errors
    /// 存储故障。
    async fn delete_env(&self, id: EnvironmentId) -> Result<bool, AppError>;

    /// 登记部署目标。
    ///
    /// # Errors
    /// 存储故障或唯一键冲突。
    async fn add_target(&self, t: &NewDeployTarget) -> Result<DeployTarget, AppError>;

    /// 环境的部署目标列表。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_targets(&self, env_id: EnvironmentId) -> Result<Vec<DeployTarget>, AppError>;

    /// 移除部署目标。
    ///
    /// # Errors
    /// 存储故障。
    async fn remove_target(&self, id: DeployTargetId) -> Result<bool, AppError>;

    /// 写入/覆盖环境变量。
    ///
    /// # Errors
    /// 存储故障。
    async fn upsert_var(&self, v: &NewEnvVar) -> Result<EnvVar, AppError>;

    /// 环境变量列表。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_vars(&self, env_id: EnvironmentId) -> Result<Vec<EnvVar>, AppError>;

    /// 删除环境变量。
    ///
    /// # Errors
    /// 存储故障。
    async fn delete_var(&self, id: EnvVarId) -> Result<bool, AppError>;

    /// 关联仓库(重复幂等)。
    ///
    /// # Errors
    /// 存储故障。
    async fn link_repo(&self, app_id: ApplicationId, repo_id: uuid::Uuid) -> Result<(), AppError>;

    /// 解除仓库关联。
    ///
    /// # Errors
    /// 存储故障。
    async fn unlink_repo(
        &self,
        app_id: ApplicationId,
        repo_id: uuid::Uuid,
    ) -> Result<bool, AppError>;

    /// 应用关联的仓库 ID 列表。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_repo_ids(&self, app_id: ApplicationId) -> Result<Vec<uuid::Uuid>, AppError>;

    /// 按仓库 ID 取展示名(`full_name`,只读投影)。
    ///
    /// # Errors
    /// 存储故障。
    async fn repo_full_names(&self, ids: &[uuid::Uuid])
    -> Result<Vec<(uuid::Uuid, String)>, AppError>;

}


// ---------- 服务规格(T-312 / PAAS-03) ----------

/// 端口声明。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PortSpec {
    /// 容器端口。
    pub container: u16,
    /// 协议(http/tcp/grpc)。
    #[serde(default = "default_protocol")]
    pub protocol: String,
}

fn default_protocol() -> String {
    "http".to_owned()
}

/// 环境变量绑定(值可引用环境变量:`${KEY}` 由部署时注入)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EnvBinding {
    /// 变量名。
    pub name: String,
    /// 值或引用。
    #[serde(default)]
    pub value: String,
}

/// 卷声明。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct VolumeSpec {
    /// 卷名(逻辑名)。
    pub name: String,
    /// 容器内挂载路径(绝对路径)。
    pub mount_path: String,
    /// 容量(MB;0 = 不限)。
    #[serde(default)]
    pub size_mb: u32,
}

/// 健康检查。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct HealthCheck {
    /// 探活路径。
    pub path: String,
    /// 端口。
    pub port: u16,
    /// 间隔秒。
    #[serde(default = "default_interval")]
    pub interval_seconds: u32,
    /// 超时秒。
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u32,
}

fn default_interval() -> u32 {
    10
}
fn default_timeout() -> u32 {
    3
}

/// 服务规格(声明式;版本化存储,不可变)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ServiceSpec {
    /// 镜像(含 tag)。
    pub image: String,
    /// 副本数。
    #[serde(default = "default_replicas")]
    pub replicas: u32,
    /// 端口。
    #[serde(default)]
    pub ports: Vec<PortSpec>,
    /// 环境变量绑定。
    #[serde(default)]
    pub env: Vec<EnvBinding>,
    /// 卷。
    #[serde(default)]
    pub volumes: Vec<VolumeSpec>,
    /// 健康检查。
    #[serde(default)]
    pub healthcheck: Option<HealthCheck>,
}

fn default_replicas() -> u32 {
    1
}

/// 副本数上限。
pub const MAX_REPLICAS: u32 = 20;

/// 规格校验:返回**全部**问题(便于一次改完,而非逐个报错)。
#[must_use]
pub fn validate_spec(spec: &ServiceSpec) -> Vec<String> {
    let mut errs = Vec::new();
    if spec.image.trim().is_empty() {
        errs.push("image 不能为空".to_owned());
    } else if !spec.image.contains(':') {
        errs.push("image 必须带 tag(如 registry/app:1.2.3)".to_owned());
    }
    if spec.replicas == 0 || spec.replicas > MAX_REPLICAS {
        errs.push(format!("replicas 须在 1~{MAX_REPLICAS}"));
    }
    let mut seen_ports = std::collections::HashSet::new();
    for p in &spec.ports {
        if p.container == 0 {
            errs.push("端口不能为 0".to_owned());
        }
        if !seen_ports.insert(p.container) {
            errs.push(format!("端口重复:{}", p.container));
        }
        if !["http", "tcp", "grpc"].contains(&p.protocol.as_str()) {
            errs.push(format!("协议非法:{}", p.protocol));
        }
    }
    let mut seen_env = std::collections::HashSet::new();
    for e in &spec.env {
        if e.name.trim().is_empty()
            || !e
                .name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_')
        {
            errs.push(format!("环境变量名非法:{}", e.name));
        }
        if !seen_env.insert(&e.name) {
            errs.push(format!("环境变量重复:{}", e.name));
        }
    }
    let mut seen_vol = std::collections::HashSet::new();
    for v in &spec.volumes {
        if !v.mount_path.starts_with('/') {
            errs.push(format!("卷挂载路径须为绝对路径:{}", v.mount_path));
        }
        if !seen_vol.insert(&v.mount_path) {
            errs.push(format!("卷挂载路径重复:{}", v.mount_path));
        }
    }
    if let Some(h) = &spec.healthcheck {
        if !h.path.starts_with('/') {
            errs.push("健康检查 path 须以 / 开头".to_owned());
        }
        if h.port == 0 {
            errs.push("健康检查端口不能为 0".to_owned());
        }
        if h.interval_seconds == 0 || h.timeout_seconds == 0 {
            errs.push("健康检查间隔/超时须大于 0".to_owned());
        }
        if h.timeout_seconds >= h.interval_seconds {
            errs.push("健康检查超时应小于间隔".to_owned());
        }
        if !spec.ports.is_empty() && !spec.ports.iter().any(|p| p.container == h.port) {
            errs.push(format!("健康检查端口 {} 未在 ports 中声明", h.port));
        }
    }
    errs
}

/// 解析规格文本(先 JSON 后 YAML;未知字段视为错误,避免拼写错误被静默忽略)。
///
/// # Errors
/// 两种格式均解析失败,或字段不合法。
pub fn parse_spec(text: &str) -> Result<ServiceSpec, AppError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid(crate::DOMAIN_CODE, "规格内容为空"));
    }
    let parsed = serde_json::from_str::<ServiceSpec>(trimmed)
        .or_else(|_| serde_yaml::from_str::<ServiceSpec>(trimmed))
        .map_err(|e| {
            AppError::invalid(crate::DOMAIN_CODE, format!("规格解析失败(JSON/YAML):{e}"))
        })?;
    let errs = validate_spec(&parsed);
    if !errs.is_empty() {
        return Err(AppError::invalid(
            crate::DOMAIN_CODE,
            format!("规格校验未通过:{}", errs.join(";")),
        ));
    }
    Ok(parsed)
}

/// 规格版本。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpecVersion {
    /// 版本号(从 1 递增)。
    pub version: i32,
    /// 规格内容。
    pub spec: ServiceSpec,
    /// 原始声明文本(规格即代码留痕)。
    pub source: String,
    /// 提交人。
    pub created_by: Option<String>,
    /// 提交时间。
    pub created_at: String,
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    fn base() -> ServiceSpec {
        ServiceSpec {
            image: "registry.internal/web:1.0.0".to_owned(),
            replicas: 2,
            ports: vec![PortSpec {
                container: 8080,
                protocol: "http".to_owned(),
            }],
            env: vec![EnvBinding {
                name: "LOG_LEVEL".to_owned(),
                value: "info".to_owned(),
            }],
            volumes: vec![VolumeSpec {
                name: "data".to_owned(),
                mount_path: "/var/data".to_owned(),
                size_mb: 1024,
            }],
            healthcheck: Some(HealthCheck {
                path: "/healthz".to_owned(),
                port: 8080,
                interval_seconds: 10,
                timeout_seconds: 3,
            }),
        }
    }

    #[test]
    fn valid_spec_passes() {
        assert!(validate_spec(&base()).is_empty());
    }

    #[test]
    fn collects_all_problems_at_once() {
        let mut s = base();
        s.image = "no-tag".to_owned();
        s.replicas = 0;
        s.ports = vec![
            PortSpec { container: 80, protocol: "http".to_owned() },
            PortSpec { container: 80, protocol: "bogus".to_owned() },
        ];
        s.volumes = vec![VolumeSpec { name: "v".to_owned(), mount_path: "relative".to_owned(), size_mb: 0 }];
        let errs = validate_spec(&s);
        assert!(errs.len() >= 5, "应一次报出全部问题,实际:{errs:?}");
        assert!(errs.iter().any(|e| e.contains("必须带 tag")));
        assert!(errs.iter().any(|e| e.contains("replicas")));
        assert!(errs.iter().any(|e| e.contains("端口重复")));
        assert!(errs.iter().any(|e| e.contains("协议非法")));
        assert!(errs.iter().any(|e| e.contains("绝对路径")));
    }

    #[test]
    fn healthcheck_must_reference_declared_port() {
        let mut s = base();
        s.healthcheck = Some(HealthCheck {
            path: "/healthz".to_owned(),
            port: 9999,
            interval_seconds: 10,
            timeout_seconds: 3,
        });
        assert!(validate_spec(&s).iter().any(|e| e.contains("未在 ports 中声明")));
    }

    #[test]
    fn healthcheck_timeout_must_be_less_than_interval() {
        let mut s = base();
        s.healthcheck = Some(HealthCheck {
            path: "/healthz".to_owned(),
            port: 8080,
            interval_seconds: 3,
            timeout_seconds: 5,
        });
        assert!(validate_spec(&s).iter().any(|e| e.contains("超时应小于间隔")));
    }

    #[test]
    fn parses_yaml_spec_as_code() {
        let yaml = "image: registry.internal/web:2.0.0\nreplicas: 3\nports:\n  - container: 8080\n    protocol: http\n";
        let spec = parse_spec(yaml).expect("YAML 应可解析");
        assert_eq!(spec.image, "registry.internal/web:2.0.0");
        assert_eq!(spec.replicas, 3);
    }

    #[test]
    fn parses_json_spec() {
        let json = r#"{"image":"app:1.0.0","replicas":1}"#;
        let spec = parse_spec(json).expect("JSON 应可解析");
        assert_eq!(spec.replicas, 1);
        assert!(spec.ports.is_empty());
    }

    #[test]
    fn rejects_unknown_field() {
        let json = r#"{"image":"app:1.0.0","replica":2}"#;
        let err = parse_spec(json).expect_err("未知字段应被拒绝");
        assert!(err.message().contains("解析失败"), "实际:{}", err.message());
    }

    #[test]
    fn rejects_invalid_spec_with_all_reasons() {
        let yaml = "image: app\nreplicas: 99\n";
        let err = parse_spec(yaml).expect_err("应校验失败");
        assert!(err.message().contains("必须带 tag"));
        assert!(err.message().contains("replicas"));
    }

    #[test]
    fn rejects_empty_text() {
        assert!(parse_spec("   ").is_err());
    }
}

/// 服务规格仓储(T-312):版本化存储,规格不可变。
#[async_trait::async_trait]
pub trait SpecRepository: Send + Sync {
    /// 取/建 服务行(应用 × 环境唯一),返回服务 ID 与当前版本。
    ///
    /// # Errors
    /// 存储故障。
    async fn ensure_service(
        &self,
        app_id: ApplicationId,
        env_id: EnvironmentId,
    ) -> Result<(ServiceId, i32), AppError>;

    /// 追加规格版本(版本号由调用方按当前版本 +1 传入)。
    ///
    /// # Errors
    /// 存储故障或版本冲突。
    async fn append_spec(
        &self,
        service_id: ServiceId,
        version: i32,
        spec: &ServiceSpec,
        source: &str,
        created_by: Option<UserId>,
    ) -> Result<(), AppError>;

    /// 更新服务当前版本指针。
    ///
    /// # Errors
    /// 存储故障。
    async fn set_current_version(
        &self,
        service_id: ServiceId,
        version: i32,
    ) -> Result<(), AppError>;

    /// 当前规格。
    ///
    /// # Errors
    /// 存储故障。
    async fn current_spec(&self, service_id: ServiceId) -> Result<Option<SpecVersion>, AppError>;

    /// 规格版本历史(新→旧)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_spec_versions(&self, service_id: ServiceId)
    -> Result<Vec<SpecVersion>, AppError>;

    /// 指定版本规格(回滚/对比用)。
    ///
    /// # Errors
    /// 存储故障。
    async fn spec_at(&self, service_id: ServiceId, version: i32)
    -> Result<Option<SpecVersion>, AppError>;
}

// ---------- 配置与密钥(T-317 / PAAS-06) ----------

define_id!(
    /// 配置集 ID(应用 × 环境 唯一)。
    ConfigId
);

/// 配置项(存储形态:`secret = true` 时 `value` 为密文)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigItem {
    /// 键。
    pub key: String,
    /// 值(敏感项为密文)。
    pub value: String,
    /// 是否敏感。
    #[serde(default)]
    pub secret: bool,
}

/// 配置项(展示形态:敏感项不回显值)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct ConfigItemView {
    /// 键。
    pub key: String,
    /// 值(敏感项为 `null`)。
    pub value: Option<String>,
    /// 是否敏感。
    pub secret: bool,
}

/// 配置版本。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigVersion {
    /// 版本号。
    pub version: i32,
    /// 配置项(展示形态)。
    pub items: Vec<ConfigItemView>,
    /// 原始声明文本。
    pub source: String,
    /// 提交人。
    pub created_by: Option<String>,
    /// 提交时间。
    pub created_at: String,
}

/// 解析配置声明(YAML/JSON 映射;值可为字符串或 `{value, secret}`)。
///
/// # Errors
/// 空文本、非映射、对象形态字段非法、键重复或键名非法。
pub fn parse_config(text: &str) -> Result<Vec<ConfigItem>, AppError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid(crate::DOMAIN_CODE, "配置内容为空"));
    }
    let value: serde_yaml::Value = serde_yaml::from_str(trimmed)
        .or_else(|_| serde_json::from_str::<serde_yaml::Value>(trimmed))
        .map_err(|e| {
            AppError::invalid(crate::DOMAIN_CODE, format!("配置解析失败(YAML/JSON):{e}"))
        })?;
    let map = match value {
        serde_yaml::Value::Mapping(m) => m,
        _ => {
            return Err(AppError::invalid(
                crate::DOMAIN_CODE,
                "配置须为键值映射(如 `KEY: value`)",
            ));
        }
    };
    let mut items: Vec<ConfigItem> = Vec::with_capacity(map.len());
    for (k, v) in map {
        let Some(key) = k.as_str().map(str::to_owned) else {
            return Err(AppError::invalid(crate::DOMAIN_CODE, "配置键须为字符串"));
        };
        if !valid_config_key(&key) {
            return Err(AppError::invalid(
                crate::DOMAIN_CODE,
                format!("配置键非法(字母/数字/下划线):{key}"),
            ));
        }
        let item = match v {
            // 标量一律按字符串处理:配置里 `PORT: 8080` / `ENABLED: true` 是常态,
            // 若只接受字符串会把最普通的写法判为非法。
            serde_yaml::Value::String(s) => ConfigItem {
                key,
                value: s,
                secret: false,
            },
            serde_yaml::Value::Number(n) => ConfigItem {
                key,
                value: n.to_string(),
                secret: false,
            },
            serde_yaml::Value::Bool(b) => ConfigItem {
                key,
                value: b.to_string(),
                secret: false,
            },
            serde_yaml::Value::Mapping(m) => {
                let mut value = String::new();
                let mut secret = false;
                for (mk, mv) in &m {
                    match mk.as_str() {
                        Some("value") => {
                            value = scalar_to_string(mv).ok_or_else(|| {
                                AppError::invalid(
                                    crate::DOMAIN_CODE,
                                    format!("{key}.value 须为标量(字符串/数字/布尔)"),
                                )
                            })?;
                        }
                        Some("secret") => {
                            secret = mv.as_bool().ok_or_else(|| {
                                AppError::invalid(crate::DOMAIN_CODE, format!("{key}.secret 须为布尔"))
                            })?;
                        }
                        other => {
                            return Err(AppError::invalid(
                                crate::DOMAIN_CODE,
                                format!("{key} 含未知字段:{other:?}(仅支持 value/secret)"),
                            ));
                        }
                    }
                }
                ConfigItem { key, value, secret }
            }
            _ => {
                return Err(AppError::invalid(
                    crate::DOMAIN_CODE,
                    format!("{key} 的值须为标量(字符串/数字/布尔)或 {{value, secret}}"),
                ));
            }
        };
        if items.iter().any(|i| i.key == item.key) {
            return Err(AppError::invalid(
                crate::DOMAIN_CODE,
                format!("配置键重复:{}", item.key),
            ));
        }
        items.push(item);
    }
    if items.is_empty() {
        return Err(AppError::invalid(crate::DOMAIN_CODE, "配置不能为空"));
    }
    items.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(items)
}

/// YAML 标量 → 字符串(字符串/数字/布尔);其余返回 None。
fn scalar_to_string(v: &serde_yaml::Value) -> Option<String> {
    match v {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// 脱敏声明文本:把敏感项的值替换为占位符。
///
/// **为什么需要**:版本留痕会保存原始声明文本,若原样入库则敏感值明文落库,
/// 与"敏感项加密存储 / 注入不落明文"冲突 —— 留痕只保留"声明了哪些键",不保留值。
#[must_use]
pub fn redact_source(source: &str, items: &[ConfigItem]) -> String {
    let mut out = source.to_owned();
    for it in items.iter().filter(|i| i.secret) {
        if !it.value.is_empty() {
            out = out.replace(&it.value, "••••••(已脱敏)");
        }
    }
    out
}

/// 配置键规则:字母/数字/下划线,1~128 位。
#[must_use]
pub fn valid_config_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

/// 存储形态的配置版本:`(version, items, source, created_by, created_at)`。
/// 其中 `items` 的敏感项为密文。
pub type StoredConfigVersion = (i32, Vec<ConfigItem>, String, Option<String>, String);

/// 配置仓储(T-317):版本化存储,敏感项密文落库。
#[async_trait::async_trait]
pub trait ConfigRepository: Send + Sync {
    /// 取/建 配置集(应用 × 环境唯一),返回配置 ID 与当前版本。
    ///
    /// # Errors
    /// 存储故障。
    async fn ensure_config(
        &self,
        app_id: ApplicationId,
        env_id: EnvironmentId,
    ) -> Result<(ConfigId, i32), AppError>;

    /// 追加配置版本。
    ///
    /// # Errors
    /// 存储故障或版本冲突。
    async fn append_config(
        &self,
        config_id: ConfigId,
        version: i32,
        items: &[ConfigItem],
        source: &str,
        created_by: Option<UserId>,
    ) -> Result<(), AppError>;

    /// 更新当前版本指针。
    ///
    /// # Errors
    /// 存储故障。
    async fn set_current_config_version(
        &self,
        config_id: ConfigId,
        version: i32,
    ) -> Result<(), AppError>;

    /// 当前配置(存储形态,含密文;解密由调用方负责)。
    ///
    /// # Errors
    /// 存储故障。
    async fn current_config(
        &self,
        config_id: ConfigId,
    ) -> Result<Option<StoredConfigVersion>, AppError>;

    /// 版本历史(新→旧,存储形态)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_config_versions(
        &self,
        config_id: ConfigId,
    ) -> Result<Vec<StoredConfigVersion>, AppError>;
}

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn parses_plain_and_secret_items() {
        let yaml = "LOG_LEVEL: info\nDB_PASSWORD:\n  value: s3cret\n  secret: true\n";
        let items = parse_config(yaml).expect("应解析成功");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].key, "DB_PASSWORD", "按键排序");
        assert!(items[0].secret);
        assert_eq!(items[0].value, "s3cret");
        assert!(!items[1].secret);
        assert_eq!(items[1].value, "info");
    }

    #[test]
    fn parses_json_map() {
        let json = r#"{"A":"1","B":{"value":"2","secret":true}}"#;
        let items = parse_config(json).expect("JSON 应可解析");
        assert_eq!(items.len(), 2);
        assert!(items[1].secret);
    }

    #[test]
    fn rejects_non_mapping() {
        assert!(parse_config("- a\n- b\n").is_err());
    }

    #[test]
    fn rejects_unknown_object_field() {
        let yaml = "K:\n  value: v\n  secred: true\n";
        let err = parse_config(yaml).expect_err("未知字段应被拒");
        assert!(err.message().contains("未知字段"), "实际:{}", err.message());
    }

    #[test]
    fn rejects_bad_key_and_non_string_value() {
        assert!(parse_config("bad-key: v\n").is_err(), "连字符不允许");
        assert!(parse_config("K: [1,2]\n").is_err(), "列表值不支持");
        assert!(parse_config("   ").is_err(), "空文本");
    }

    #[test]
    fn redact_source_removes_secret_values() {
        let source = "LOG_LEVEL: info\nDB_PASSWORD:\n  value: SuperSecret123\n  secret: true\n";
        let items = parse_config(source).expect("解析");
        let redacted = redact_source(source, &items);
        assert!(!redacted.contains("SuperSecret123"), "敏感值不得残留在留痕中");
        assert!(redacted.contains("DB_PASSWORD"), "键名保留,便于审计");
        assert!(redacted.contains("info"), "非敏感值保留");
    }

    #[test]
    fn coerces_scalar_values_to_string() {
        let items = parse_config("PORT: 8080\nENABLED: true\nNAME: web\n").expect("标量应可解析");
        let get = |k: &str| items.iter().find(|i| i.key == k).map(|i| i.value.clone());
        assert_eq!(get("PORT").as_deref(), Some("8080"), "数字按字符串保存");
        assert_eq!(get("ENABLED").as_deref(), Some("true"));
        assert_eq!(get("NAME").as_deref(), Some("web"));
    }

    #[test]
    fn coerces_scalar_inside_object_form() {
        let items = parse_config("RETRIES:\n  value: 3\n  secret: false\n").expect("应解析");
        assert_eq!(items[0].value, "3");
        assert!(!items[0].secret);
    }

    #[test]
    fn redact_source_is_noop_without_secrets() {
        let source = "A: 1\nB: 2\n";
        let items = parse_config(source).expect("解析");
        assert_eq!(redact_source(source, &items), source);
    }

    #[test]
    fn config_key_rule() {
        assert!(valid_config_key("DB_HOST"));
        assert!(valid_config_key("a1_b2"));
        assert!(!valid_config_key(""));
        assert!(!valid_config_key("has-dash"));
        assert!(!valid_config_key(&"x".repeat(129)));
    }
}

// ---------- 域名绑定(T-315 / PAAS-05) ----------

define_id!(
    /// 域名绑定 ID。
    DomainId
);

/// 域名来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DomainKind {
    /// 平台自动分配。
    Auto,
    /// 用户自定义。
    Custom,
}

impl DomainKind {
    /// 存储值。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Custom => "custom",
        }
    }

    /// 解析。
    #[must_use]
    pub fn parse(v: &str) -> Option<Self> {
        match v {
            "auto" => Some(Self::Auto),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

/// 域名绑定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDomain {
    /// 绑定 ID。
    pub id: DomainId,
    /// 所属环境。
    pub env_id: EnvironmentId,
    /// 主机名。
    pub host: String,
    /// 来源。
    pub kind: DomainKind,
    /// 是否启用 TLS。
    pub tls_enabled: bool,
    /// 证书引用(T-316 签发后回填)。
    pub cert_ref: String,
    /// 绑定时间。
    pub created_at: DateTime<Utc>,
}

/// 新建绑定入参。
#[derive(Debug, Clone)]
pub struct NewDomain {
    /// 所属环境。
    pub env_id: EnvironmentId,
    /// 主机名。
    pub host: String,
    /// 来源。
    pub kind: DomainKind,
    /// 是否启用 TLS。
    pub tls_enabled: bool,
}

/// 域名仓储。
#[async_trait::async_trait]
pub trait DomainRepository: Send + Sync {
    /// 绑定(主机名全局唯一,冲突返回 `APP-409`)。
    ///
    /// # Errors
    /// 存储故障或唯一键冲突。
    async fn bind(&self, d: &NewDomain) -> Result<AppDomain, AppError>;

    /// 环境的域名列表。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_by_env(&self, env_id: EnvironmentId) -> Result<Vec<AppDomain>, AppError>;

    /// 按主机名查(判重与冲突提示用)。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_by_host(&self, host: &str) -> Result<Option<AppDomain>, AppError>;

    /// 解绑,返回是否命中。
    ///
    /// # Errors
    /// 存储故障。
    async fn unbind(&self, id: DomainId) -> Result<bool, AppError>;

    /// 回填证书引用(T-316 签发后调用)。
    ///
    /// # Errors
    /// 存储故障。
    async fn set_cert_ref(&self, id: DomainId, cert_ref: &str) -> Result<(), AppError>;
}

// ---------- 证书(T-316 / PAAS-07) ----------

define_id!(
    /// 证书 ID。
    CertificateId
);

/// 证书记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Certificate {
    /// 证书 ID。
    pub id: CertificateId,
    /// 所属环境。
    pub env_id: EnvironmentId,
    /// 证书引用(网关按 `{cert_ref}.crt/.key` 定位文件)。
    pub cert_ref: String,
    /// 覆盖域名。
    pub domains: Vec<String>,
    /// 签发方。
    pub provider: String,
    /// 状态:issued/expiring/expired/failed。
    pub status: String,
    /// 到期时间(CA 未提供时为空)。
    pub not_after: Option<DateTime<Utc>>,
    /// 签发时间。
    pub issued_at: DateTime<Utc>,
}

/// 证书仓储。
#[async_trait::async_trait]
pub trait CertificateRepository: Send + Sync {
    /// 写入证书记录(同 `cert_ref` 覆盖更新,续期即覆盖)。
    ///
    /// # Errors
    /// 存储故障。
    async fn upsert_cert(
        &self,
        env_id: EnvironmentId,
        cert_ref: &str,
        domains: &[String],
        provider: &str,
        status: &str,
        not_after: Option<DateTime<Utc>>,
    ) -> Result<Certificate, AppError>;

    /// 环境的证书列表(新→旧)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_certs(&self, env_id: EnvironmentId) -> Result<Vec<Certificate>, AppError>;

    /// 到期时间早于 `before` 的证书(续期扫描;`not_after` 为空视为需要续期)。
    ///
    /// # Errors
    /// 存储故障。
    async fn expiring_before(
        &self,
        before: DateTime<Utc>,
    ) -> Result<Vec<Certificate>, AppError>;

    /// 按引用查。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_cert_by_ref(&self, cert_ref: &str) -> Result<Option<Certificate>, AppError>;
}

// ---------- 部署主机(T-311 / PAAS-03) ----------

define_id!(
    /// 部署主机 ID。
    DeployHostId
);

/// 主机状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostStatus {
    /// 已登记但 agent 未注册。
    Pending,
    /// agent 心跳正常。
    Online,
    /// 曾注册但心跳超时。
    Offline,
}

impl HostStatus {
    /// 存储值。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Online => "online",
            Self::Offline => "offline",
        }
    }

    /// 解析。
    #[must_use]
    pub fn parse(v: &str) -> Option<Self> {
        match v {
            "pending" => Some(Self::Pending),
            "online" => Some(Self::Online),
            "offline" => Some(Self::Offline),
            _ => None,
        }
    }
}

/// 部署主机。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeployHost {
    /// 主机 ID。
    pub id: DeployHostId,
    /// 平台侧登记名(唯一)。
    pub name: String,
    /// 是否已被 agent 注册。
    pub registered: bool,
    /// agent 上报的主机名。
    pub hostname: String,
    /// agent 版本。
    pub agent_version: String,
    /// 标签。
    pub labels: Vec<String>,
    /// 在线状态(按最后心跳实时判定)。
    pub status: HostStatus,
    /// 最后心跳时间。
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    /// 登记时间。
    pub created_at: DateTime<Utc>,
}

/// 心跳间隔(秒;平台建议值)。
pub const HEARTBEAT_INTERVAL_SECS: i64 = 30;
/// 超过该倍数 × 间隔未心跳即判离线。
pub const OFFLINE_AFTER_INTERVALS: i64 = 3;

/// 部署目标是否"心跳新鲜":仅当最近一次心跳在 1 个间隔内才认为可安全投递。
///
/// 放宽到离线宽限(3× 间隔)会把**刚掉线、仍在宽限期内**的主机选为部署目标,
/// 命令投递失败后部署只能靠超时收敛 —— 部署下发的门槛要比"在线判定"更严。
#[must_use]
pub fn is_fresh_for_deploy(
    status: HostStatus,
    last_heartbeat_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> bool {
    if status != HostStatus::Online {
        return false;
    }
    let Some(at) = last_heartbeat_at else {
        return false;
    };
    now - at <= chrono::Duration::seconds(HEARTBEAT_INTERVAL_SECS)
}

/// 由最后心跳时间判定在线状态(纯函数,便于单测)。
#[must_use]
pub fn status_from_heartbeat(
    registered: bool,
    last_heartbeat_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> HostStatus {
    if !registered {
        return HostStatus::Pending;
    }
    let Some(at) = last_heartbeat_at else {
        return HostStatus::Offline;
    };
    let grace = chrono::Duration::seconds(HEARTBEAT_INTERVAL_SECS * OFFLINE_AFTER_INTERVALS);
    if now - at <= grace {
        HostStatus::Online
    } else {
        HostStatus::Offline
    }
}

/// 主机仓储。
#[async_trait::async_trait]
pub trait DeployHostRepository: Send + Sync {
    /// 登记主机(名冲突返回 `APP-409`),返回主机与**一次性注册令牌**。
    ///
    /// # Errors
    /// 存储故障或唯一键冲突。
    async fn create_host(
        &self,
        name: &str,
        token_hash: &str,
        labels: &[String],
    ) -> Result<DeployHost, AppError>;

    /// 全部主机(新→旧)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_hosts(&self) -> Result<Vec<DeployHost>, AppError>;

    /// 按注册令牌哈希查主机(agent 注册用)。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_by_token_hash(&self, token_hash: &str) -> Result<Option<DeployHost>, AppError>;

    /// 按 ID 查主机。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_host(&self, id: DeployHostId) -> Result<Option<DeployHost>, AppError>;

    /// 标记 agent 已注册并写入上报信息(令牌随即失效)。
    ///
    /// # Errors
    /// 存储故障。
    async fn mark_registered(
        &self,
        id: DeployHostId,
        hostname: &str,
        agent_version: &str,
        labels: &[String],
    ) -> Result<(), AppError>;

    /// 记录心跳。
    ///
    /// # Errors
    /// 存储故障。
    async fn touch_heartbeat(&self, id: DeployHostId) -> Result<(), AppError>;
}

#[cfg(test)]
mod host_tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("合法时间")
    }

    #[test]
    fn unregistered_host_is_pending() {
        assert_eq!(status_from_heartbeat(false, None, now()), HostStatus::Pending);
        // 未注册即使有心跳时间也仍为 pending(防御异常数据)
        assert_eq!(
            status_from_heartbeat(false, Some(now()), now()),
            HostStatus::Pending
        );
    }

    #[test]
    fn recent_heartbeat_is_online() {
        let at = now() - chrono::Duration::seconds(30);
        assert_eq!(status_from_heartbeat(true, Some(at), now()), HostStatus::Online);
        // 刚好在宽限内(3 × 30s = 90s)
        let edge = now() - chrono::Duration::seconds(90);
        assert_eq!(status_from_heartbeat(true, Some(edge), now()), HostStatus::Online);
    }

    #[test]
    fn stale_heartbeat_is_offline() {
        let at = now() - chrono::Duration::seconds(91);
        assert_eq!(status_from_heartbeat(true, Some(at), now()), HostStatus::Offline);
        assert_eq!(status_from_heartbeat(true, None, now()), HostStatus::Offline);
    }

    #[test]
    fn status_roundtrip() {
        for s in [HostStatus::Pending, HostStatus::Online, HostStatus::Offline] {
            assert_eq!(HostStatus::parse(s.as_str()), Some(s));
        }
        assert_eq!(HostStatus::parse("nope"), None);
    }
}

// ---------- 部署执行(T-313 / PAAS-04) ----------

define_id!(
    /// 部署记录 ID。
    DeploymentId
);

/// 部署状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentStatus {
    /// 已下发,等待回执。
    Pending,
    /// 全部主机成功。
    Success,
    /// 任一主机失败。
    Failure,
}

impl DeploymentStatus {
    /// 存储值。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Success => "success",
            Self::Failure => "failure",
        }
    }

    /// 解析。
    #[must_use]
    pub fn parse(v: &str) -> Option<Self> {
        match v {
            "pending" => Some(Self::Pending),
            "success" => Some(Self::Success),
            "failure" => Some(Self::Failure),
            _ => None,
        }
    }
}

/// 部署记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Deployment {
    /// 部署 ID。
    pub id: DeploymentId,
    /// 所属环境。
    pub env_id: EnvironmentId,
    /// 应用键。
    pub app_key: String,
    /// 使用的规格版本。
    pub spec_version: Option<i32>,
    /// 状态。
    pub status: DeploymentStatus,
    /// 目标主机 ID。
    pub target_hosts: Vec<DeployHostId>,
    /// 各主机回执(主机 ID → 文本结果)。
    pub results: std::collections::BTreeMap<String, String>,
    /// 说明。
    pub detail: String,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 结束时间。
    pub finished_at: Option<DateTime<Utc>>,
}

/// 回执超时阈值(分钟):超过则判定失败并给出原因。
pub const DEPLOY_TIMEOUT_MINUTES: i64 = 5;

/// 由各主机回执汇总部署状态(纯函数,便于单测)。
#[must_use]
pub fn aggregate_status(
    targets: &[DeployHostId],
    results: &std::collections::BTreeMap<String, String>,
) -> DeploymentStatus {
    if targets.is_empty() {
        return DeploymentStatus::Failure;
    }
    // 全部回执到齐才算终结;未到齐保持 pending
    if targets
        .iter()
        .any(|t| !results.contains_key(&t.as_uuid().to_string()))
    {
        return DeploymentStatus::Pending;
    }
    if results.values().all(|v| v.starts_with("OK")) {
        DeploymentStatus::Success
    } else {
        DeploymentStatus::Failure
    }
}

/// agent 命令投递口(实现方在组合根:L4 持有 gRPC 通道,域内不依赖 tonic)。
#[async_trait::async_trait]
pub trait AgentCommandSink: Send + Sync {
    /// 向主机投递命令,返回命令 ID;主机不在线返回 `Ok(None)`。
    ///
    /// # Errors
    /// 序列化/内部故障。
    async fn push(
        &self,
        host_id: DeployHostId,
        kind: &str,
        payload_json: &str,
    ) -> Result<Option<String>, AppError>;
}

/// 部署仓储。
#[async_trait::async_trait]
pub trait DeploymentRepository: Send + Sync {
    /// 新建部署记录。
    ///
    /// # Errors
    /// 存储故障。
    async fn create_deployment(
        &self,
        env_id: EnvironmentId,
        app_key: &str,
        spec_version: Option<i32>,
        target_hosts: &[DeployHostId],
        detail: &str,
    ) -> Result<Deployment, AppError>;

    /// 登记命令 → 部署映射(供回执路由)。
    ///
    /// # Errors
    /// 存储故障。
    async fn record_command(
        &self,
        command_id: uuid::Uuid,
        deployment_id: DeploymentId,
        host_id: DeployHostId,
        kind: &str,
    ) -> Result<(), AppError>;

    /// 按命令 ID 找部署。
    ///
    /// # Errors
    /// 存储故障。
    async fn deployment_by_command(
        &self,
        command_id: uuid::Uuid,
    ) -> Result<Option<(Deployment, DeployHostId)>, AppError>;

    /// 写入某主机回执并刷新状态。
    ///
    /// # Errors
    /// 存储故障。
    async fn record_result(
        &self,
        deployment_id: DeploymentId,
        host_id: DeployHostId,
        result: &str,
    ) -> Result<Deployment, AppError>;

    /// 环境的部署记录(新→旧)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_deployments(
        &self,
        env_id: EnvironmentId,
        limit: i64,
    ) -> Result<Vec<Deployment>, AppError>;

    /// 按 ID 取。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_deployment(&self, id: DeploymentId) -> Result<Option<Deployment>, AppError>;

    /// 收敛超时未回执的部署:把早于 `before` 仍为 `pending` 的标记为失败。
    ///
    /// 为什么需要:host 收到命令后掉线(agent 重启/网络断)会让部署**永远挂着**,
    /// 上层无法判断到底成没成 —— 运维工具不能有这种悬空状态。
    ///
    /// # Errors
    /// 存储故障。
    async fn expire_pending(&self, before: DateTime<Utc>) -> Result<usize, AppError>;
}

#[cfg(test)]
mod deployment_tests {
    use super::*;

    fn host(n: u128) -> DeployHostId {
        DeployHostId::from_uuid(uuid::Uuid::from_u128(n))
    }

    fn results(pairs: &[(u128, &str)]) -> std::collections::BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(n, v)| (host(*n).as_uuid().to_string(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn pending_until_all_report() {
        let targets = vec![host(1), host(2)];
        assert_eq!(
            aggregate_status(&targets, &results(&[])),
            DeploymentStatus::Pending
        );
        assert_eq!(
            aggregate_status(&targets, &results(&[(1, "OK 1/1 容器就绪")])),
            DeploymentStatus::Pending,
            "只有一台回执时仍为进行中"
        );
    }

    #[test]
    fn success_when_all_ok() {
        let targets = vec![host(1), host(2)];
        let r = results(&[(1, "OK 1/1 容器就绪"), (2, "OK 1/1 容器就绪")]);
        assert_eq!(aggregate_status(&targets, &r), DeploymentStatus::Success);
    }

    #[test]
    fn failure_when_any_fails() {
        let targets = vec![host(1), host(2)];
        let r = results(&[(1, "OK 1/1 容器就绪"), (2, "ERR 拉取镜像失败")]);
        assert_eq!(aggregate_status(&targets, &r), DeploymentStatus::Failure);
    }

    #[test]
    fn no_targets_is_failure() {
        assert_eq!(
            aggregate_status(&[], &results(&[])),
            DeploymentStatus::Failure
        );
    }

    #[test]
    fn deploy_target_requires_fresh_heartbeat() {
        let now = DateTime::from_timestamp(1_700_000_000, 0).expect("合法时间");
        // 刚心跳:可投递
        assert!(is_fresh_for_deploy(
            HostStatus::Online,
            Some(now - chrono::Duration::seconds(10)),
            now
        ));
        // 超过 1 个间隔但仍"在线"(宽限期内):**不**作为部署目标
        assert!(!is_fresh_for_deploy(
            HostStatus::Online,
            Some(now - chrono::Duration::seconds(60)),
            now
        ));
        assert!(!is_fresh_for_deploy(HostStatus::Offline, Some(now), now));
        assert!(!is_fresh_for_deploy(HostStatus::Online, None, now));
    }

    #[test]
    fn status_roundtrip() {
        for s in [
            DeploymentStatus::Pending,
            DeploymentStatus::Success,
            DeploymentStatus::Failure,
        ] {
            assert_eq!(DeploymentStatus::parse(s.as_str()), Some(s));
        }
    }
}
