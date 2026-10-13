//! 敏感配置的对称加密(T-317 / PAAS-06)。
//!
//! 算法:AES-256-GCM(认证加密,篡改可检出)。存储格式 `base64(nonce ‖ ciphertext ‖ tag)`,
//! 每条独立随机 nonce(12 字节),同一明文多次加密密文不同。
//!
//! 主密钥来自 `CODEPLAN_CONFIG_KEY`(base64 的 32 字节);未配置时退化为
//! **固定的开发密钥**并在启动时告警 —— 生产必须显式配置,否则换密钥后旧密文不可解。

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine as _;
use codeplan_common::error::AppError;

use crate::DOMAIN_CODE;

/// nonce 长度(GCM 标准 12 字节)。
const NONCE_LEN: usize = 12;
/// 开发默认密钥(仅本地开发;生产必须用 `CODEPLAN_CONFIG_KEY` 覆盖)。
const DEV_KEY_SEED: &str = "codeplan-dev-config-key-not-for-production";

/// 主密钥。
pub struct ConfigKey {
    key: [u8; 32],
    /// 是否使用了开发默认密钥(用于启动告警与接口提示)。
    pub is_dev_key: bool,
}

impl ConfigKey {
    /// 从环境变量加载;缺失时用开发默认密钥。
    #[must_use]
    pub fn from_env() -> Self {
        match std::env::var("CODEPLAN_CONFIG_KEY") {
            Ok(raw) if !raw.trim().is_empty() => {
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(raw.trim())
                    .ok()
                    .filter(|b| b.len() == 32);
                match decoded {
                    Some(bytes) => {
                        let mut key = [0u8; 32];
                        key.copy_from_slice(&bytes);
                        Self {
                            key,
                            is_dev_key: false,
                        }
                    }
                    None => {
                        eprintln!(
                            "[appcenter] CODEPLAN_CONFIG_KEY 非法(需 base64 的 32 字节),已回退开发密钥"
                        );
                        Self::dev()
                    }
                }
            }
            _ => Self::dev(),
        }
    }

    /// 开发默认密钥(由固定种子派生,保证本地重启后旧密文仍可解)。
    #[must_use]
    pub fn dev() -> Self {
        use sha2::{Digest as _, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(DEV_KEY_SEED.as_bytes());
        let out = hasher.finalize();
        let mut key = [0u8; 32];
        key.copy_from_slice(&out);
        Self {
            key,
            is_dev_key: true,
        }
    }

    /// 显式指定密钥(测试用)。
    #[must_use]
    pub fn from_bytes(key: [u8; 32]) -> Self {
        Self {
            key,
            is_dev_key: false,
        }
    }

    /// 加密 → `base64(nonce ‖ ciphertext)`。
    ///
    /// # Errors
    /// 加密失败(理论上仅内存不足)。
    pub fn encrypt(&self, plaintext: &str) -> Result<String, AppError> {
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.key));
        let nonce_bytes: [u8; NONCE_LEN] = rand::random();
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string())))?;
        let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ct);
        Ok(base64::engine::general_purpose::STANDARD.encode(out))
    }

    /// 解密。
    ///
    /// # Errors
    /// 编码非法、长度不足、密钥不匹配或密文被篡改(认证失败)。
    pub fn decrypt(&self, encoded: &str) -> Result<String, AppError> {
        let raw = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!("密文编码非法")))?;
        if raw.len() <= NONCE_LEN {
            return Err(AppError::internal(DOMAIN_CODE)
                .with_details(serde_json::json!("密文长度不足")));
        }
        let (nonce_bytes, ct) = raw.split_at(NONCE_LEN);
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.key));
        let plain = cipher
            .decrypt(Nonce::from_slice(nonce_bytes), ct)
            .map_err(|_| {
                AppError::internal(DOMAIN_CODE)
                    .with_details(serde_json::json!("密文解密失败(密钥不匹配或已损坏)"))
            })?;
        String::from_utf8(plain).map_err(|_| {
            AppError::internal(DOMAIN_CODE).with_details(serde_json::json!("明文非 UTF-8"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> ConfigKey {
        ConfigKey::from_bytes([7u8; 32])
    }

    #[test]
    fn roundtrip_recovers_plaintext() {
        let k = key();
        let ct = k.encrypt("s3cret-value").expect("加密应成功");
        assert_ne!(ct, "s3cret-value", "密文不得等于明文");
        assert_eq!(k.decrypt(&ct).expect("解密应成功"), "s3cret-value");
    }

    #[test]
    fn same_plaintext_yields_different_ciphertext() {
        let k = key();
        let a = k.encrypt("same").expect("加密");
        let b = k.encrypt("same").expect("加密");
        assert_ne!(a, b, "随机 nonce 保证每次密文不同");
        assert_eq!(k.decrypt(&a).expect("解密"), k.decrypt(&b).expect("解密"));
    }

    #[test]
    fn wrong_key_fails_to_decrypt() {
        let ct = key().encrypt("secret").expect("加密");
        let other = ConfigKey::from_bytes([9u8; 32]);
        assert!(other.decrypt(&ct).is_err(), "换密钥必须解密失败");
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let k = key();
        let ct = k.encrypt("secret").expect("加密");
        let mut raw = base64::engine::general_purpose::STANDARD
            .decode(&ct)
            .expect("解码");
        let last = raw.len() - 1;
        raw[last] ^= 0x01; // 翻转末字节
        let bad = base64::engine::general_purpose::STANDARD.encode(raw);
        assert!(k.decrypt(&bad).is_err(), "GCM 认证应检出篡改");
    }

    #[test]
    fn rejects_malformed_input() {
        let k = key();
        assert!(k.decrypt("not-base64!!!").is_err());
        assert!(k.decrypt(&base64::engine::general_purpose::STANDARD.encode([0u8; 4])).is_err());
    }

    #[test]
    fn dev_key_is_deterministic() {
        let a = ConfigKey::dev();
        let b = ConfigKey::dev();
        let ct = a.encrypt("x").expect("加密");
        assert_eq!(b.decrypt(&ct).expect("开发密钥可跨重启解密"), "x");
        assert!(a.is_dev_key);
    }
}
