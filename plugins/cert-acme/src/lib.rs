//! cert-acme 官方插件(T-316 / PAAS-07):ACME v2 签发 + DNS-01 校验。
//!
//! 分工:**插件只负责"向 CA 拿到证书"**;证书的存储、`cert_ref` 回填与
//! 部署钩子由平台侧完成(见 appcenter)。
//!
//! ACME 协议细节复用 `instant-acme`(社区维护的 ACME v2 客户端),
//! 本插件只实现平台相关的部分:DNS-01 记录下发与账号凭据持久化。

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use codeplan_ext::{CertIssuer, IssuedCert};
use instant_acme::{
    Account, AccountBuilder, AccountCredentials, ChallengeType, Identifier, NewAccount, NewOrder,
    OrderStatus,
};

/// DNS-01 校验记录的前缀。
pub const DNS01_PREFIX: &str = "_acme-challenge";

/// 安装 rustls 的进程级 CryptoProvider(幂等)。
///
/// **必须在任何 TLS 客户端创建之前调用**:依赖树里同时存在 aws-lc-rs 与 ring 时,
/// rustls 无法自动选定 provider,会在首次握手时 panic
/// (`Could not automatically determine the process-level CryptoProvider`)。
pub fn install_crypto_provider() {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    }
}

/// 从 PEM 证书链解析到期时间(RFC 3339)。
///
/// 证书链第一段即叶证书;`not_after` 缺失会导致续期任务无法判断是否到期,
/// 故解析失败按错误处理而不是静默留空。
///
/// # Errors
/// PEM 无法解析或缺少有效期。
pub fn cert_not_after(cert_pem: &str) -> Result<String, String> {
    use x509_parser::prelude::*;
    // 链首即叶证书:x509-parser 的 parse_x509_pem 直接读第一个 PEM 块
    let (_, pem) = x509_parser::pem::parse_x509_pem(cert_pem.as_bytes())
        .map_err(|e| format!("证书链无法解析为 PEM:{e}"))?;
    let (_, cert) = X509Certificate::from_der(&pem.contents)
        .map_err(|e| format!("证书 DER 解析失败:{e}"))?;
    let ts = cert.validity().not_after.timestamp();
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| d.to_rfc3339())
        .ok_or_else(|| "证书有效期时间戳非法".to_owned())
}

/// 计算 DNS-01 记录名:`_acme-challenge.<域名>`(泛域名去掉 `*.`)。
#[must_use]
pub fn dns01_record_name(domain: &str) -> String {
    let bare = domain.trim().trim_start_matches("*.").trim_end_matches('.');
    format!("{DNS01_PREFIX}.{bare}")
}

/// 下发/清理 DNS-01 TXT 记录的能力(可插拔:文件、测试服务、云厂商 API)。
#[async_trait]
pub trait DnsSolver: Send + Sync {
    /// 写入 TXT 记录。
    ///
    /// # Errors
    /// 下发失败(凭据/网络/权限)。
    async fn set_txt(&self, record: &str, value: &str) -> Result<(), String>;

    /// 清理 TXT 记录(签发结束后调用,失败不致命)。
    ///
    /// # Errors
    /// 清理失败。
    async fn clear_txt(&self, record: &str) -> Result<(), String>;
}

/// 文件下发器:把 TXT 记录写进 JSON 文件。
///
/// 适合两类场景:① 无 DNS API 的内网环境,由运维脚本读取后写入权威 DNS;
/// ② 本地 e2e(测试侧轮询该文件后写入测试 DNS 服务)。
pub struct FileSolver {
    path: PathBuf,
}

impl FileSolver {
    /// 构造。
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn read(&self) -> std::collections::BTreeMap<String, String> {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn write(&self, map: &std::collections::BTreeMap<String, String>) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let body = serde_json::to_string_pretty(map).map_err(|e| e.to_string())?;
        std::fs::write(&self.path, body).map_err(|e| e.to_string())
    }
}

#[async_trait]
impl DnsSolver for FileSolver {
    async fn set_txt(&self, record: &str, value: &str) -> Result<(), String> {
        let mut map = self.read();
        map.insert(record.to_owned(), value.to_owned());
        self.write(&map)
    }

    async fn clear_txt(&self, record: &str) -> Result<(), String> {
        let mut map = self.read();
        map.remove(record);
        self.write(&map)
    }
}

/// HTTP 下发器:向 `{base}/set-txt`、`{base}/clear-txt` POST。
///
/// 适配 pebble-challtestsrv 与常见 DNS 网关。
pub struct HttpSolver {
    base: String,
    token: Option<String>,
}

impl HttpSolver {
    /// 构造。
    #[must_use]
    pub fn new(base: impl Into<String>, token: Option<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_owned(),
            token,
        }
    }

    async fn post(&self, path: &str, body: serde_json::Value) -> Result<(), String> {
        let mut req = reqwest::Client::new()
            .post(format!("{}{}", self.base, path))
            .json(&body);
        if let Some(t) = &self.token {
            req = req.header("Authorization", format!("Bearer {t}"));
        }
        let resp = req.send().await.map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("{path} 返回 {}", resp.status()));
        }
        Ok(())
    }
}

#[async_trait]
impl DnsSolver for HttpSolver {
    async fn set_txt(&self, record: &str, value: &str) -> Result<(), String> {
        self.post(
            "/set-txt",
            serde_json::json!({"host": record, "value": value}),
        )
        .await
    }

    async fn clear_txt(&self, record: &str) -> Result<(), String> {
        self.post("/clear-txt", serde_json::json!({"host": record}))
            .await
    }
}

/// ACME 签发器。
pub struct AcmeCertIssuer {
    directory_url: String,
    account_key: PathBuf,
    /// 自定义 CA 根证书路径(测试 CA/私有 CA;为空则用系统信任库)。
    ca_root: Option<PathBuf>,
    solver: Box<dyn DnsSolver>,
    /// 下发记录后等待传播的秒数(DNS 生效有延迟)。
    propagation_wait_secs: u64,
}

impl AcmeCertIssuer {
    /// 构造。
    #[must_use]
    pub fn new(
        directory_url: impl Into<String>,
        account_key: impl Into<PathBuf>,
        solver: Box<dyn DnsSolver>,
        propagation_wait_secs: u64,
    ) -> Self {
        Self {
            directory_url: directory_url.into(),
            account_key: account_key.into(),
            ca_root: None,
            solver,
            propagation_wait_secs,
        }
    }

    /// 指定自定义 CA 根证书(本地/私有 CA 场景,如 pebble)。
    #[must_use]
    pub fn with_ca_root(mut self, pem: impl Into<PathBuf>) -> Self {
        self.ca_root = Some(pem.into());
        self
    }

    /// 构造 ACME 客户端:`ca_root` 存在时信任该根,否则用系统信任库。
    fn builder(&self) -> Result<AccountBuilder, String> {
        match &self.ca_root {
            Some(pem) => Account::builder_with_root(pem)
                .map_err(|e| format!("加载 CA 根证书失败({}):{e}", pem.display())),
            None => Account::builder().map_err(|e| format!("初始化 ACME 客户端失败:{e}")),
        }
    }

    /// 按环境变量构造(未启用时返回 `None`)。
    ///
    /// `CODEPLAN_ACME_DIRECTORY`(必填)、`CODEPLAN_ACME_ACCOUNT_KEY`(账户凭据路径)、
    /// `CODEPLAN_ACME_DNS_FILE`(文件下发器)或 `CODEPLAN_ACME_DNS_API`(HTTP 下发器)。
    #[must_use]
    pub fn from_env(data_dir: &Path) -> Option<Self> {
        let directory = std::env::var("CODEPLAN_ACME_DIRECTORY").ok()?;
        if directory.trim().is_empty() {
            return None;
        }
        let account_key = std::env::var("CODEPLAN_ACME_ACCOUNT_KEY")
            .map(PathBuf::from)
            .unwrap_or_else(|_| data_dir.join("acme/account.json"));
        let solver: Box<dyn DnsSolver> = match std::env::var("CODEPLAN_ACME_DNS_API") {
            Ok(api) if !api.trim().is_empty() => Box::new(HttpSolver::new(
                api,
                std::env::var("CODEPLAN_ACME_DNS_TOKEN").ok(),
            )),
            _ => Box::new(FileSolver::new(
                std::env::var("CODEPLAN_ACME_DNS_FILE")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| data_dir.join("acme/dns01.json")),
            )),
        };
        let wait = std::env::var("CODEPLAN_ACME_PROPAGATION_WAIT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);
        let mut issuer = Self::new(directory, account_key, solver, wait);
        if let Ok(root) = std::env::var("CODEPLAN_ACME_CA_ROOT")
            && !root.trim().is_empty()
        {
            issuer = issuer.with_ca_root(root);
        }
        Some(issuer)
    }

    /// 载入或创建 ACME 账号(凭据持久化,避免每次签发新建账号触发限流)。
    async fn account(&self) -> Result<Account, String> {
        if let Ok(raw) = std::fs::read_to_string(&self.account_key)
            && let Ok(creds) = serde_json::from_str::<AccountCredentials>(&raw)
        {
            return self
                .builder()?
                .from_credentials(creds)
                .await
                .map_err(|e| format!("载入 ACME 账号失败:{e}"));
        }
        let (account, creds) = self
            .builder()?
            .create(
                &NewAccount {
                    contact: &[],
                    terms_of_service_agreed: true,
                    only_return_existing: false,
                },
                self.directory_url.clone(),
                None,
            )
            .await
            .map_err(|e| format!("创建 ACME 账号失败:{e}"))?;
        if let Some(parent) = self.account_key.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let body = serde_json::to_string_pretty(&creds).map_err(|e| e.to_string())?;
        std::fs::write(&self.account_key, body).map_err(|e| format!("写入账号凭据失败:{e}"))?;
        Ok(account)
    }
}

#[async_trait]
impl CertIssuer for AcmeCertIssuer {
    fn name(&self) -> &'static str {
        "cert-acme"
    }

    async fn issue(&self, domains: &[String]) -> Result<IssuedCert, String> {
        if domains.is_empty() {
            return Err("未提供域名".to_owned());
        }
        let account = self.account().await?;
        let identifiers: Vec<Identifier> =
            domains.iter().map(|d| Identifier::Dns(d.clone())).collect();
        let mut order = account
            .new_order(&NewOrder::new(&identifiers))
            .await
            .map_err(|e| format!("创建订单失败:{e}"))?;

        // DNS-01:逐条授权下发 TXT
        let mut records: Vec<String> = Vec::new();
        {
            let mut auths = order.authorizations();
            while let Some(auth) = auths.next().await {
                let mut auth = auth.map_err(|e| format!("读取授权失败:{e}"))?;
                let ident = auth.identifier().to_string();
                let Some(mut challenge) = auth.challenge(ChallengeType::Dns01) else {
                    return Err(format!("{ident} 未提供 DNS-01 挑战"));
                };
                let record = dns01_record_name(&ident);
                let value = challenge.key_authorization().dns_value();
                self.solver.set_txt(&record, &value).await?;
                records.push(record);
                challenge
                    .set_ready()
                    .await
                    .map_err(|e| format!("通知挑战就绪失败:{e}"))?;
            }
        }

        if self.propagation_wait_secs > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(self.propagation_wait_secs)).await;
        }

        // 轮询订单直到 Ready
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            let status = order
                .refresh()
                .await
                .map_err(|e| format!("刷新订单失败:{e}"))?
                .status;
            match status {
                OrderStatus::Ready => break,
                OrderStatus::Invalid => return Err("订单被 CA 判定无效(挑战未通过)".to_owned()),
                _ => {
                    if std::time::Instant::now() > deadline {
                        return Err("等待挑战通过超时".to_owned());
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }

        // 生成密钥与 CSR 并 finalize(返回私钥 PEM)
        let key_pem = order
            .finalize()
            .await
            .map_err(|e| format!("提交 CSR 失败:{e}"))?;

        // 取证书链
        let mut cert_pem = None;
        for _ in 0..30 {
            match order.certificate().await {
                Ok(Some(pem)) => {
                    cert_pem = Some(pem);
                    break;
                }
                Ok(None) => tokio::time::sleep(std::time::Duration::from_secs(2)).await,
                Err(e) => return Err(format!("下载证书失败:{e}")),
            }
        }
        for r in records {
            let _ = self.solver.clear_txt(&r).await;
        }
        let cert_pem = cert_pem.ok_or_else(|| "证书签发超时(CA 未返回证书)".to_owned())?;
        let not_after = cert_not_after(&cert_pem)?;
        Ok(IssuedCert {
            cert_pem,
            key_pem,
            not_after: Some(not_after),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns01_record_name_strips_wildcard() {
        assert_eq!(
            dns01_record_name("web.example.com"),
            "_acme-challenge.web.example.com"
        );
        assert_eq!(
            dns01_record_name("*.example.com"),
            "_acme-challenge.example.com",
            "泛域名去掉 *."
        );
        assert_eq!(
            dns01_record_name("example.com."),
            "_acme-challenge.example.com",
            "去掉末尾点"
        );
    }

    #[tokio::test]
    async fn file_solver_roundtrip() {
        let dir = std::env::temp_dir().join(format!("cp-acme-{}", unique_suffix()));
        let path = dir.join("dns01.json");
        let s = FileSolver::new(&path);
        s.set_txt("_acme-challenge.a.example.com", "v1")
            .await
            .expect("写入");
        s.set_txt("_acme-challenge.b.example.com", "v2")
            .await
            .expect("写入");
        let raw = std::fs::read_to_string(&path).expect("读取");
        assert!(raw.contains("v1") && raw.contains("v2"));
        s.clear_txt("_acme-challenge.a.example.com")
            .await
            .expect("清理");
        let raw = std::fs::read_to_string(&path).expect("读取");
        assert!(!raw.contains("v1"), "清理后不应残留");
        assert!(raw.contains("v2"), "其他记录不受影响");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_not_after_from_pem() {
        // 用 rcgen 生成一张自签证书,验证有效期解析(不依赖外部 CA)
        let key = rcgen::KeyPair::generate().expect("密钥");
        let params = rcgen::CertificateParams::new(vec!["a.example.com".to_owned()]).expect("参数");
        let cert = params.self_signed(&key).expect("自签");
        let pem = cert.pem();
        let not_after = cert_not_after(&pem).expect("应解析出有效期");
        assert!(not_after.contains('T'), "应为 RFC3339:{not_after}");
        assert!(cert_not_after("not a pem").is_err(), "非法 PEM 应报错");
    }

    #[test]
    fn installs_crypto_provider_idempotently() {
        install_crypto_provider();
        install_crypto_provider(); // 重复调用不应 panic
        assert!(rustls::crypto::CryptoProvider::get_default().is_some());
    }

    #[test]
    fn from_env_disabled_without_directory() {
        // 未设置 CODEPLAN_ACME_DIRECTORY 时插件不启用
        assert!(AcmeCertIssuer::from_env(Path::new("/tmp")).is_none());
    }

    fn unique_suffix() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        format!(
            "{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        )
    }
}
