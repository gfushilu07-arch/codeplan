//! 提交签名验证(T-210):GPG(sequoia,纯 Rust crypto)+ SSH(ssh-keygen -Y)。
//!
//! git 提交签名存于 `gpgsig` 头(SSH 签名同用此头,armor 为 SSHSIG 形态);
//! 验签对象 = 去掉 `gpgsig`/`gpgsig-sha256` 头后的完整提交内容。

use std::sync::Arc;

use codeplan_common::error::AppError;
use codeplan_kernel::authn::SshKeyDirectoryAll;
use codeplan_kernel::git::GitAccess;
use codeplan_kernel::identity::CurrentUser;
use codeplan_kernel::ids::UserId;
use codeplan_kernel::lookup::IdentityLookup;
use serde_json::json;

use crate::DOMAIN_CODE;

/// 提交签名验证服务。
pub struct SigningService {
    git: Arc<dyn GitAccess>,
    gpg: Arc<dyn codeplan_kernel::authn::GpgKeyDirectory>,
    ssh: Arc<dyn SshKeyDirectoryAll>,
    ssh_dir: Arc<dyn codeplan_kernel::authn::SshKeyDirectory>,
    #[allow(dead_code)]
    identity: Arc<dyn IdentityLookup>,
}

/// 验证结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyOutcome {
    /// 是否通过。
    pub verified: bool,
    /// 签名方式:gpg / ssh / none。
    pub method: &'static str,
    /// 签名者(平台用户,可空)。
    pub signer: Option<UserId>,
    /// 密钥指纹。
    pub fingerprint: Option<String>,
}

impl SigningService {
    /// 构造。
    #[must_use]
    pub fn new(
        git: Arc<dyn GitAccess>,
        gpg: Arc<dyn codeplan_kernel::authn::GpgKeyDirectory>,
        ssh: Arc<dyn SshKeyDirectoryAll>,
        ssh_dir: Arc<dyn codeplan_kernel::authn::SshKeyDirectory>,
        identity: Arc<dyn IdentityLookup>,
    ) -> Self {
        Self {
            git,
            gpg,
            ssh,
            ssh_dir,
            identity,
        }
    }

    /// 验证某提交的签名。
    ///
    /// # Errors
    /// 仓库/提交不存在 `GIT-404`;git 故障 `GIT-500`。
    pub async fn verify_commit(
        &self,
        _actor: &CurrentUser,
        full_name: &str,
        sha: &str,
    ) -> Result<VerifyOutcome, AppError> {
        let raw = self
            .git
            .raw_commit(full_name, sha)
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(json!(e)))?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "提交不存在"))?;
        let Some((payload, sig, is_ssh)) = split_gpgsig(&raw) else {
            return Ok(VerifyOutcome {
                verified: false,
                method: "none",
                signer: None,
                fingerprint: None,
            });
        };

        if is_ssh {
            return self.verify_ssh(&payload, &sig, &raw).await;
        }
        self.verify_gpg(&payload, &sig).await
    }

    /// GPG:sequoia 验签,按提交者邮箱匹配平台用户公钥。
    async fn verify_gpg(&self, payload: &[u8], sig: &[u8]) -> Result<VerifyOutcome, AppError> {
        let committer_email = committer_email(payload);
        let keys = self
            .gpg
            .all_keys()
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(json!(e)))?;
        let mut result = crate::signing_gpg::verify(payload, sig, &keys);
        if let Some((uid, fp)) = result.take() {
            // 邮箱一致才算平台归属(公钥里声明的邮箱须含提交者邮箱)
            let email_ok = committer_email
                .as_ref()
                .is_some_and(|email| crate::signing_gpg::key_covers_email(&keys, &fp, email));
            if !email_ok {
                return Ok(VerifyOutcome {
                    verified: false,
                    method: "gpg",
                    signer: None,
                    fingerprint: Some(fp),
                });
            }
            return Ok(VerifyOutcome {
                verified: true,
                method: "gpg",
                signer: Some(uid),
                fingerprint: Some(fp),
            });
        }
        Ok(VerifyOutcome {
            verified: false,
            method: "gpg",
            signer: None,
            fingerprint: None,
        })
    }

    /// SSH:`ssh-keygen -Y verify`,principal = 提交者邮箱,密钥来自平台 SSH Key。
    async fn verify_ssh(
        &self,
        payload: &[u8],
        sig: &[u8],
        raw: &str,
    ) -> Result<VerifyOutcome, AppError> {
        let Some(committer_email) = committer_email(raw.as_bytes()) else {
            return Ok(VerifyOutcome {
                verified: false,
                method: "ssh",
                signer: None,
                fingerprint: None,
            });
        };
        let all = self
            .ssh
            .all_keys()
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(json!(e)))?;
        eprintln!(
            "[signing] ssh verify: principal={committer_email} keys={} sig_head={:?}",
            all.len(),
            String::from_utf8_lossy(&sig[..sig.len().min(40)])
        );
        let (ok, used_fingerprint) =
            crate::signing_ssh::verify(payload, sig, &committer_email, &all);
        let signer = if ok {
            let Some(fp) = used_fingerprint.clone() else {
                return Ok(VerifyOutcome {
                    verified: true,
                    method: "ssh",
                    signer: None,
                    fingerprint: None,
                });
            };
            self.ssh_dir
                .user_by_fingerprint(&fp)
                .await
                .ok()
                .flatten()
                .map(|u| u.user_id)
        } else {
            None
        };
        Ok(VerifyOutcome {
            verified: ok,
            method: "ssh",
            signer,
            fingerprint: used_fingerprint,
        })
    }
}

/// 拆出 (验签 payload, 签名块, 是否 SSH);无签名返回 None。
#[must_use]
pub fn split_gpgsig(raw: &str) -> Option<(Vec<u8>, Vec<u8>, bool)> {
    // 定位 gpgsig 头(行首 `gpgsig `)
    let (header_start, after_key) = if let Some(i) = raw.find("\ngpgsig ") {
        (i + 1, i + 1 + "gpgsig ".len())
    } else if raw.starts_with("gpgsig ") {
        (0, "gpgsig ".len())
    } else {
        return None;
    };
    let rest = &raw[after_key..];
    // 首行 + 以空格开头的 continuation 行(剥一个前导空格);
    // consumed 按原始行计(cont 行含前导空格),否则 payload 混入块尾垃圾
    let mut sig_lines: Vec<String> = Vec::new();
    let mut consumed = 0usize;
    for (i, line) in rest.split('\n').enumerate() {
        if i == 0 {
            sig_lines.push(line.to_owned());
            consumed += line.len() + 1;
        } else if let Some(stripped) = line.strip_prefix(' ') {
            sig_lines.push(stripped.to_owned());
            consumed += line.len() + 1;
        } else {
            break;
        }
    }
    // payload = 原文挖掉 "gpgsig <块>\n";raw[..header_start] 已含前一头换行
    let mut payload = String::with_capacity(raw.len());
    payload.push_str(&raw[..header_start]);
    payload.push_str(&raw[after_key + consumed..]);
    let sig_text = sig_lines.join("\n");
    let is_ssh = sig_text.contains("BEGIN SSH SIGNATURE");
    Some((payload.into_bytes(), sig_text.into_bytes(), is_ssh))
}

/// 从提交 payload 取提交者邮箱。
#[must_use]
fn committer_email(payload: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(payload);
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("committer ")
            && let (Some(start), Some(rel_end)) = (rest.find('<'), rest.find('>'))
            && rel_end > start
        {
            return Some(rest[start + 1..rel_end].to_owned());
        }
        if line.is_empty() {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const GPGSIG: &str = "tree 5a1f\nparent 0000\nauthor A <a@corp.test> 1 +0000\ncommitter A <a@corp.test> 1 +0000\ngpgsig -----BEGIN PGP SIGNATURE-----\n iQE=~\n-----END PGP SIGNATURE-----\n\nmessage body\n";

    #[test]
    fn split_extracts_sig_and_payload() {
        let (payload, sig, is_ssh) = split_gpgsig(GPGSIG).expect("应有签名");
        assert!(!is_ssh);
        let text = String::from_utf8(payload).unwrap();
        assert!(text.starts_with("tree 5a1f\n"), "payload 以原头开始");
        assert!(text.contains("committer A"));
        assert!(!text.contains("gpgsig"), "payload 不含签名头");
        assert!(text.ends_with("\n\nmessage body\n"));
        assert!(
            String::from_utf8(sig)
                .unwrap()
                .contains("BEGIN PGP SIGNATURE")
        );
    }

    #[test]
    fn split_detects_sshsig() {
        let raw = GPGSIG.replace("PGP SIGNATURE", "SSH SIGNATURE");
        let (_, _, is_ssh) = split_gpgsig(&raw).unwrap();
        assert!(is_ssh);
    }

    #[test]
    fn split_none_when_unsigned() {
        assert!(
            split_gpgsig("tree 1\nauthor A <a@x> 1 +0000\ncommitter A <a@x> 1 +0000\n\nhi\n")
                .is_none()
        );
    }

    #[test]
    fn committer_email_extracted() {
        let (payload, _, _) = split_gpgsig(GPGSIG).unwrap();
        assert_eq!(committer_email(&payload).as_deref(), Some("a@corp.test"));
    }
}
