//! macOS 运行时代码签名：给安装到 ~/.monet/bin 的辅助二进制签名。
//!
//! 优先级：已带稳定 DR 的预签（build.sh 产物，签名内嵌证书随文件拷贝依然
//! 有效，不依赖本机钥匙串）> 本机自签证书重签（scripts/setup-signing.sh
//! 创建）> adhoc 兜底（arm64 未签名不可执行）。
//! 稳定 DR = identifier+证书 形式，跨版本不变，TCC 授权不随更新失效；
//! adhoc 的 DR 退化为 cdhash，每次重编译授权即失效。

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

const IDENTITY: &str = "Monet Signing";

/// 目标已带有效签名且 DR 非 cdhash（即 identifier+证书 稳定形态）
fn has_stable_signature(path: &Path) -> bool {
    let intact = Command::new("codesign")
        .arg("--verify")
        .arg(path)
        .output()
        .is_ok_and(|o| o.status.success());
    if !intact {
        return false;
    }
    Command::new("codesign")
        .args(["-d", "-r-"])
        .arg(path)
        .output()
        .is_ok_and(|o| {
            let mut req = String::from_utf8_lossy(&o.stdout).into_owned();
            req.push_str(&String::from_utf8_lossy(&o.stderr));
            o.status.success() && req.contains("designated =>") && !req.contains("cdhash")
        })
}

/// 本机是否存在自签签名身份（进程内缓存探测结果）
fn identity_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        Command::new("security")
            .args(["find-identity", "-v", "-p", "codesigning"])
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(IDENTITY))
    })
}

/// 解锁专用钥匙串（重启后处于锁定态）。失败时调用方必须放弃证书路径——
/// 对锁定钥匙串跑 codesign 会弹出用户无法回答的密码对话框
fn unlock_keychain() -> bool {
    let Some(home) = std::env::var_os("HOME") else { return false };
    let home = std::path::PathBuf::from(home);
    // 签名基建是机器级产物，路径与 setup-signing.sh 保持硬编码一致，
    // 不随 MONET_DATA_DIR 漂移
    let pass_file = home.join(".monet/signing/keychain-password");
    let keychain = home.join("Library/Keychains/monet-signing.keychain-db");
    let Ok(pass) = std::fs::read_to_string(&pass_file) else { return false };
    Command::new("security")
        .arg("unlock-keychain")
        .arg("-p")
        .arg(pass.trim())
        .arg(&keychain)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// 给二进制签名。预签保留 > 证书重签 > adhoc 兜底
pub fn sign(path: &Path, identifier: &str) {
    sign_with_entitlements(path, identifier, None);
}

/// 带 entitlements 的签名变体：hardened runtime 下 TCC 敏感能力
/// （如 Apple Events）必须随签名授予，重签丢失 entitlements 会让
/// 已授权用户静默失去弹窗资格
pub fn sign_with_entitlements(path: &Path, identifier: &str, entitlements_xml: Option<&str>) {
    if has_stable_signature(path) {
        return;
    }
    if identity_available() {
        if unlock_keychain() {
            let entitlements_file = entitlements_xml.and_then(|xml| {
                let file = std::env::temp_dir().join(format!(
                    "monet-entitlements-{}.plist",
                    std::process::id()
                ));
                std::fs::write(&file, xml).ok().map(|_| file)
            });
            let mut args: Vec<std::ffi::OsString> = [
                "--force",
                "--options",
                "runtime",
                "--sign",
                IDENTITY,
                "--identifier",
                identifier,
            ]
            .iter()
            .map(Into::into)
            .collect();
            if let Some(file) = &entitlements_file {
                args.push("--entitlements".into());
                args.push(file.into());
            }
            let out = Command::new("codesign").args(&args).arg(path).output();
            if let Some(file) = &entitlements_file {
                let _ = std::fs::remove_file(file);
            }
            match out {
                Ok(o) if o.status.success() => return,
                Ok(o) => log::warn!(
                    "certificate signing failed for {}, falling back to adhoc: {}",
                    path.display(),
                    String::from_utf8_lossy(&o.stderr).trim()
                ),
                Err(e) => log::warn!("codesign spawn failed for {}: {}", path.display(), e),
            }
        } else {
            log::warn!(
                "signing keychain locked/unavailable, falling back to adhoc for {}",
                path.display()
            );
        }
    }
    let _ = Command::new("codesign")
        .args(["--force", "--sign", "-"])
        .arg(path)
        .output();
}
