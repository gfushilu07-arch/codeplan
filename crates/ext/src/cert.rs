//! 证书签发扩展点(T-316):官方 `cert-acme` 等实现此 trait。
//!
//! 平台只依赖本契约:签发细节(ACME/DNS-01/商业 CA)留在插件内,
//! 证书的**存储、回填与部署钩子**由平台侧完成。

use async_trait::async_trait;

/// 签发结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedCert {
    /// 证书链(PEM)。
    pub cert_pem: String,
    /// 私钥(PEM)。
    pub key_pem: String,
    /// 到期时间(RFC 3339;签发方未提供则为空)。
    pub not_after: Option<String>,
}

/// 证书签发器:对一组域名签发一张证书(通常用 DNS-01 校验,支持泛域名)。
#[async_trait]
pub trait CertIssuer: Send + Sync {
    /// 实现名(如 `cert-acme`),用于审计与按实现禁用。
    fn name(&self) -> &'static str;

    /// 为 `domains` 签发证书。
    ///
    /// # Errors
    /// 校验未通过、CA 拒绝、网络失败等;错误文本面向运维,应包含可行动信息。
    async fn issue(&self, domains: &[String]) -> Result<IssuedCert, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Stub;

    #[async_trait]
    impl CertIssuer for Stub {
        fn name(&self) -> &'static str {
            "stub"
        }
        async fn issue(&self, domains: &[String]) -> Result<IssuedCert, String> {
            if domains.is_empty() {
                return Err("无域名".to_owned());
            }
            Ok(IssuedCert {
                cert_pem: "CERT".to_owned(),
                key_pem: "KEY".to_owned(),
                not_after: None,
            })
        }
    }

    #[tokio::test]
    async fn trait_object_is_usable() {
        let issuer: Box<dyn CertIssuer> = Box::new(Stub);
        assert_eq!(issuer.name(), "stub");
        assert!(issuer.issue(&[]).await.is_err());
        let ok = issuer.issue(&["a.example.com".to_owned()]).await.expect("签发");
        assert_eq!(ok.cert_pem, "CERT");
    }
}
