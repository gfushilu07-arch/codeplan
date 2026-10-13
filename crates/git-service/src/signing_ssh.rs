//! SSH 提交签名验证(ssh-keygen -Y verify,allowed_signers 动态构建)。

use std::io::Write as _;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};

use codeplan_kernel::authn::SshPublicKeyEntry;

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 验证 SSHSIG 签名;返回 (是否通过, 命中的公钥指纹)。
#[must_use]
pub fn verify(
    payload: &[u8],
    sig_armor: &[u8],
    principal: &str,
    keys: &[SshPublicKeyEntry],
) -> (bool, Option<String>) {
    let dir = std::env::temp_dir().join(format!(
        "codeplan-ssh-verify-{}-{}",
        std::process::id(),
        TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    if std::fs::create_dir_all(&dir).is_err() {
        return (false, None);
    }
    let payload_path = dir.join("payload");
    let sig_path = dir.join("sig");
    let allowed_path = dir.join("allowed_signers");

    let write = |path: &std::path::Path, data: &[u8]| std::fs::write(path, data).is_ok();
    if !write(&payload_path, payload) || !write(&sig_path, sig_armor) {
        let _ = std::fs::remove_dir_all(&dir);
        return (false, None);
    }
    // 调试转储(定位 e2e 差异;确认后移除)
    let _ = std::fs::copy(&payload_path, "/tmp/t210_srv_payload.bin");
    let _ = std::fs::copy(&sig_path, "/tmp/t210_srv_sig.txt");
    let _ = std::fs::copy(&allowed_path, "/tmp/t210_srv_allowed");
    // allowed_signers:principal 取提交者邮箱,逐行放一个平台公钥
    let mut allowed = String::new();
    for k in keys {
        allowed.push_str(principal);
        allowed.push(' ');
        allowed.push_str(k.public_key.trim());
        allowed.push('\n');
    }
    if !write(&allowed_path, allowed.as_bytes()) {
        let _ = std::fs::remove_dir_all(&dir);
        return (false, None);
    }

    let out = std::process::Command::new("ssh-keygen")
        .args(["-Y", "verify"])
        .arg("-f")
        .arg(&allowed_path)
        .args(["-I", principal])
        .args(["-n", "git"])
        .arg("-s")
        .arg(&sig_path)
        .stdin(Stdio::from(
            std::fs::File::open(&payload_path).expect("payload 已写入"),
        ))
        .output();
    let _ = std::fs::remove_dir_all(&dir);
    match out {
        Ok(o) => {
            eprintln!(
                "[signing-ssh] exit={:?} stderr={}",
                o.status.code(),
                String::from_utf8_lossy(&o.stderr).trim()
            );
            if o.status.success() {
                // 从 stderr 提取命中指纹("Signing key fingerprint: SHA256:...")
                let text = String::from_utf8_lossy(&o.stderr);
                let fp = text
                    .lines()
                    .find_map(|l| l.strip_prefix("Signing key fingerprint: "))
                    .map(str::to_owned);
                (true, fp)
            } else {
                (false, None)
            }
        }
        Err(e) => {
            eprintln!("[signing-ssh] spawn 失败: {e}");
            (false, None)
        }
    }
}

/// 供测试/工具:确认写文件助手存在(防误删)。
#[allow(dead_code)]
fn _write_smoke() -> bool {
    let mut buf = std::env::temp_dir();
    buf.push("codeplan-ssh-verify-smoke");
    let ok = std::fs::File::create(&buf)
        .and_then(|mut f| f.write_all(b"1"))
        .is_ok();
    let _ = std::fs::remove_file(&buf);
    ok
}
