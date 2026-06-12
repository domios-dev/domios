//! Install bundled CLI binary to ~/.codeterm/bin/ + PATH integration.

use tauri::Manager;

/// Compute hex-encoded sha256 of a file. Returns `None` on I/O error.
pub(crate) fn file_sha256(path: &std::path::Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).ok()?;
    let bytes = hasher.finalize();
    Some(bytes.iter().map(|b| format!("{:02x}", b)).collect())
}

/// Install bundled CLI binary to ~/.codeterm/bin/ on startup (runs in background).
pub(crate) fn install_bundled_cli(app: &tauri::App) {
    let bin_name = if cfg!(windows) {
        "codeterm.exe"
    } else {
        "codeterm"
    };

    let resource_path = app.path().resolve(
        format!("cli/{}", bin_name),
        tauri::path::BaseDirectory::Resource,
    );

    let src = match resource_path {
        Ok(p) if p.exists() => p,
        _ => {
            log::debug!("[cli-install] No bundled CLI found (dev mode?), skipping");
            return;
        }
    };

    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            log::error!("[cli-install] Cannot determine home directory");
            return;
        }
    };

    // Run in background to not block app startup
    std::thread::spawn(move || {
        let dest_dir = home.join(".codeterm").join("bin");
        let dest = dest_dir.join(bin_name);

        // Skip iff destination already matches the bundle by content (sha256), not size. Two different release builds can produce same-sized binaries
        // with different content; size-only check silently leaves a stale CLI in PATH (the v1.1.5→1.1.6 incident: same 7334912 bytes, different hashes, mesh signing broke).
        if dest.exists() {
            match (file_sha256(&src), file_sha256(&dest)) {
                (Some(src_hash), Some(dest_hash)) if src_hash == dest_hash => {
                    log::debug!(
                        "[cli-install] CLI already up-to-date (sha256={}), skipping",
                        &src_hash[..12]
                    );
                    return;
                }
                (Some(src_hash), Some(dest_hash)) => {
                    log::info!(
                        "[cli-install] CLI hash mismatch — overwriting (src={}, dest={})",
                        &src_hash[..12],
                        &dest_hash[..12]
                    );
                }
                _ => {
                    log::warn!("[cli-install] Could not hash existing CLI — proceeding with copy");
                }
            }
        }

        if let Err(e) = std::fs::create_dir_all(&dest_dir) {
            log::error!("[cli-install] Failed to create dir: {}", e);
            return;
        }

        match std::fs::copy(&src, &dest) {
            Ok(bytes) => {
                let hash_short = file_sha256(&dest)
                    .map(|h| h[..12].to_string())
                    .unwrap_or_else(|| "?".into());
                log::info!(
                    "[cli-install] Installed CLI ({} bytes, sha256={}) to {}",
                    bytes,
                    hash_short,
                    dest.display()
                );
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
                    // Repair stale ~/.local/bin symlinks (e.g. old launcher) so PATH calls hit this install.
                    // Otherwise PATH dispatch can no-op by re-entering the daemon launcher.
                    let local_bin = home.join(".local").join("bin");
                    let _ = std::fs::create_dir_all(&local_bin);
                    let link = local_bin.join("codeterm");
                    let needs_create = match std::fs::symlink_metadata(&link) {
                        Ok(meta) if meta.file_type().is_symlink() => {
                            match std::fs::read_link(&link) {
                                Ok(target) if target == dest => false,
                                Ok(stale) => {
                                    log::warn!(
                                        "[cli-install] Replacing stale symlink {} -> {}",
                                        link.display(),
                                        stale.display()
                                    );
                                    let _ = std::fs::remove_file(&link);
                                    true
                                }
                                Err(_) => {
                                    let _ = std::fs::remove_file(&link);
                                    true
                                }
                            }
                        }
                        Ok(_) => {
                            // Regular file or directory — user/installer placed a real binary here. Don't clobber; log and skip.
                            log::warn!(
                                "[cli-install] {} exists and is not a symlink; refusing to replace",
                                link.display()
                            );
                            false
                        }
                        Err(_) => true,
                    };
                    if needs_create {
                        if let Err(e) = std::os::unix::fs::symlink(&dest, &link) {
                            log::debug!("[cli-install] Symlink to ~/.local/bin failed: {}", e);
                        }
                    }
                }
                #[cfg(windows)]
                add_to_user_path(&dest_dir);
            }
            Err(e) => log::error!("[cli-install] Failed to copy CLI: {}", e),
        }
    });
}

/// Add a directory to the Windows User PATH (persistent, no admin needed).
#[cfg(windows)]
fn add_to_user_path(dir: &std::path::Path) {
    let dir_str = dir.to_string_lossy().replace('/', "\\");
    let mut q = std::process::Command::new("reg");
    q.args(["query", "HKCU\\Environment", "/v", "Path"]);
    crate::process_util::no_window_std(&mut q);
    let output = q.output();
    let current = match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
        Err(_) => String::new(),
    };
    if current.contains(&*dir_str) {
        return;
    }
    let current_val = current
        .lines()
        .find(|l| l.contains("REG_") && l.contains("Path"))
        .and_then(|l| {
            l.split("REG_EXPAND_SZ")
                .nth(1)
                .or_else(|| l.split("REG_SZ").nth(1))
        })
        .map(|v| v.trim().to_string())
        .unwrap_or_default();
    let new_val = if current_val.is_empty() {
        dir_str.to_string()
    } else {
        format!("{};{}", current_val, dir_str)
    };
    let mut a = std::process::Command::new("reg");
    a.args([
        "add",
        "HKCU\\Environment",
        "/v",
        "Path",
        "/t",
        "REG_EXPAND_SZ",
        "/d",
        &new_val,
        "/f",
    ]);
    crate::process_util::no_window_std(&mut a);
    let result = a.output();
    if let Ok(o) = result {
        if o.status.success() {
            log::info!("[cli-install] Added {} to User PATH", dir_str);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::file_sha256;
    use std::io::Write;

    #[test]
    fn hash_distinguishes_same_size_different_content() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.bin");
        let b = dir.path().join("b.bin");
        // Two files of identical length but different content — exactly the v1.1.5/1.1.6 failure mode that defeated the old size-only skip.
        std::fs::File::create(&a)
            .unwrap()
            .write_all(&[0u8; 64])
            .unwrap();
        std::fs::File::create(&b)
            .unwrap()
            .write_all(&[1u8; 64])
            .unwrap();
        let ha = file_sha256(&a).expect("hash a");
        let hb = file_sha256(&b).expect("hash b");
        assert_eq!(
            std::fs::metadata(&a).unwrap().len(),
            std::fs::metadata(&b).unwrap().len()
        );
        assert_ne!(
            ha, hb,
            "same-size files with different content must hash differently"
        );
    }

    #[test]
    fn hash_is_deterministic_for_same_content() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c.bin");
        std::fs::File::create(&p)
            .unwrap()
            .write_all(b"codeterm-cli-fixture")
            .unwrap();
        assert_eq!(file_sha256(&p).unwrap(), file_sha256(&p).unwrap());
    }

    #[test]
    fn hash_returns_none_on_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist.bin");
        assert!(file_sha256(&missing).is_none());
    }
}
