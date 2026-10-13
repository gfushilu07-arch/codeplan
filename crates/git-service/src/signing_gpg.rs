//! GPG 验签(sequoia-openpgp,纯 Rust crypto 后端)。

use codeplan_kernel::authn::GpgKeyEntry;
use codeplan_kernel::ids::UserId;
use sequoia_openpgp::packet::Signature;
use sequoia_openpgp::parse::Parse as _;
use sequoia_openpgp::policy::StandardPolicy;
use sequoia_openpgp::{Cert, Packet, PacketPile};

/// 解析 armor 公钥的 OpenPGP 指纹。
///
/// # Errors
/// 非 OpenPGP 公钥。
pub fn cert_fingerprint(armor: &str) -> Result<String, String> {
    let cert = Cert::from_bytes(armor.as_bytes()).map_err(|e| e.to_string())?;
    Ok(cert.fingerprint().to_string())
}

/// 公钥声明的邮箱(User ID 中的 `<email>`)。
#[must_use]
pub fn cert_emails(armor: &str) -> Vec<String> {
    let Ok(cert) = Cert::from_bytes(armor.as_bytes()) else {
        return Vec::new();
    };
    cert.userids()
        .filter_map(|u| u.userid().email().ok().flatten().map(|e| e.to_owned()))
        .collect()
}

/// 公钥是否覆盖某邮箱。
#[must_use]
pub fn key_covers_email(keys: &[GpgKeyEntry], fingerprint: &str, email: &str) -> bool {
    keys.iter()
        .find(|k| {
            // 存储标识是 armor 的 SHA-256;这里以 armor 内容比对
            sequoia_openpgp::Cert::from_bytes(k.armor.as_bytes())
                .map(|c| c.fingerprint().to_string())
                .ok()
                .as_deref()
                == Some(fingerprint)
        })
        .is_some_and(|k| {
            cert_emails(&k.armor)
                .iter()
                .any(|e| e.eq_ignore_ascii_case(email))
        })
        || keys.iter().any(|k| {
            cert_emails(&k.armor)
                .iter()
                .any(|e| e.eq_ignore_ascii_case(email))
        })
}

/// 验证内嵌签名;返回 (命中用户的 key_id, OpenPGP 指纹)。
#[must_use]
pub fn verify(
    payload: &[u8],
    sig_packets: &[u8],
    keys: &[GpgKeyEntry],
) -> Option<(UserId, String)> {
    let policy = StandardPolicy::new();
    // 从二进制包流中取第一个 Signature 包
    let pile = PacketPile::from_bytes(sig_packets).ok()?;
    let sig: Signature = pile.children().find_map(|p| match p {
        Packet::Signature(s) => Some(s.clone()),
        _ => None,
    })?;
    for key in keys {
        let Ok(cert) = Cert::from_bytes(key.armor.as_bytes()) else {
            continue;
        };
        let fp = cert.fingerprint().to_string();
        for ka in cert
            .keys()
            .with_policy(&policy, None)
            .alive()
            .revoked(false)
        {
            if sig.verify_message(ka.key(), payload).is_ok() {
                return Some((key.user_id, fp));
            }
        }
    }
    None
}
