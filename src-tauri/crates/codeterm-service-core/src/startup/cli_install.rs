//! Install bundled CLI binary to the active data-dir bin/ + prod PATH integration.

/// Compute hex-encoded sha256 of a file. Returns `None` on I/O error.
pub(crate) fn file_sha256(path: &std::path::Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).ok()?;
    let bytes = hasher.finalize();
    Some(bytes.iter().map(|b| format!("{:02x}", b)).collect())
}

fn cli_bin_name() -> &'static str {
    if cfg!(windows) {
        "codeterm.exe"
    } else {
        "codeterm"
    }
}

fn bundled_cli_resource_names() -> Vec<String> {
    #[cfg(all(windows, target_arch = "x86_64"))]
    let target_name = Some("cli/codeterm-x86_64-pc-windows-msvc.exe");
    #[cfg(all(windows, target_arch = "aarch64"))]
    let target_name = Some("cli/codeterm-aarch64-pc-windows-msvc.exe");
    #[cfg(all(windows, target_arch = "x86"))]
    let target_name = Some("cli/codeterm-i686-pc-windows-msvc.exe");
    #[cfg(not(windows))]
    let target_name: Option<&str> = None;

    // The release stage ships only the plain name; an upgrade can leave the old triple-named
    // copy behind, and preferring it pins a stale CLI the server refuses to run.
    std::iter::once(format!("cli/{}", cli_bin_name()))
        .chain(target_name.map(str::to_string))
        .collect()
}

#[cfg(windows)]
fn windows_pe_machine(path: &std::path::Path) -> std::io::Result<u16> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(path)?;
    let mut dos = [0u8; 64];
    file.read_exact(&mut dos)?;
    if &dos[..2] != b"MZ" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "missing DOS header",
        ));
    }
    let pe_offset = u32::from_le_bytes(dos[60..64].try_into().unwrap()) as u64;
    file.seek(SeekFrom::Start(pe_offset))?;
    let mut header = [0u8; 6];
    file.read_exact(&mut header)?;
    if &header[..4] != b"PE\0\0" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "missing PE header",
        ));
    }
    Ok(u16::from_le_bytes([header[4], header[5]]))
}

#[cfg(windows)]
fn bundled_cli_matches_host(path: &std::path::Path) -> bool {
    #[cfg(target_arch = "x86_64")]
    const EXPECTED: u16 = 0x8664;
    #[cfg(target_arch = "aarch64")]
    const EXPECTED: u16 = 0xaa64;
    #[cfg(target_arch = "x86")]
    const EXPECTED: u16 = 0x014c;

    match windows_pe_machine(path) {
        Ok(machine) if machine == EXPECTED => true,
        Ok(machine) => {
            log::error!(
                "[cli-install] refusing incompatible bundled CLI {} (PE machine=0x{:04x}, expected=0x{:04x})",
                path.display(),
                machine,
                EXPECTED
            );
            false
        }
        Err(error) => {
            log::error!(
                "[cli-install] refusing invalid bundled CLI {}: {}",
                path.display(),
                error
            );
            false
        }
    }
}

#[cfg(not(windows))]
fn bundled_cli_matches_host(_path: &std::path::Path) -> bool {
    true
}

fn should_install_bundled_cli(is_dev: bool) -> bool {
    !is_dev
}

/// Resolve the bundled CLI inside the app's Resources. None in dev (no bundle).
pub fn resolve_bundled_cli_src(
    shell: &dyn crate::ports::host::Shell,
) -> Option<std::path::PathBuf> {
    let resource_dir = shell.resource_dir().ok()?;
    bundled_cli_resource_names().into_iter().find_map(|name| {
        let path = resource_dir.join(name);
        (path.exists() && bundled_cli_matches_host(&path)).then_some(path)
    })
}

/// The boundary every persistent user-PATH addition crosses. `refresh_installed_cli`
/// only ever hands a directory to this, so a test that supplies its own
/// implementation cannot reach `HKCU\Environment` at all.
pub(crate) trait UserPathWriter {
    fn add_dir(&self, dir: &std::path::Path);
}

struct SystemUserPath;

impl UserPathWriter for SystemUserPath {
    fn add_dir(&self, dir: &std::path::Path) {
        codeterm_util::user_path::add_to_user_path(dir);
    }
}

/// Copy the bundled CLI (`src`) to the active data-dir `bin/` iff it differs by content
/// (sha256, not size — same-size builds can differ, e.g. the v1.1.5→1.1.6
/// incident that broke mesh signing). Repairs the `~/.local/bin` symlink on
/// unix for prod installs only. Returns `Ok(true)` when it (re)installed, `Ok(false)` when already
/// up-to-date. Pure filesystem logic so it is unit-testable without Tauri.
pub(crate) fn refresh_installed_cli(
    src: &std::path::Path,
    home: &std::path::Path,
    data_dir: &std::path::Path,
) -> std::io::Result<bool> {
    refresh_installed_cli_with(src, home, data_dir, &SystemUserPath)
}

pub(crate) fn refresh_installed_cli_with(
    src: &std::path::Path,
    home: &std::path::Path,
    data_dir: &std::path::Path,
    user_path: &dyn UserPathWriter,
) -> std::io::Result<bool> {
    let bin_name = cli_bin_name();
    let dest_dir = data_dir.join("bin");
    let dest = dest_dir.join(bin_name);
    let prod_dest_dir = home.join(".domios").join("bin");
    let is_prod_dest = dest_dir == prod_dest_dir;
    let mut needs_copy = true;

    if dest.exists() {
        match (file_sha256(src), file_sha256(&dest)) {
            (Some(src_hash), Some(dest_hash)) if src_hash == dest_hash => {
                log::debug!(
                    "[cli-install] CLI already up-to-date (sha256={}), skipping",
                    &src_hash[..12.min(src_hash.len())]
                );
                needs_copy = false;
            }
            (Some(src_hash), Some(dest_hash)) => {
                log::info!(
                    "[cli-install] CLI hash mismatch — overwriting (src={}, dest={})",
                    &src_hash[..12.min(src_hash.len())],
                    &dest_hash[..12.min(dest_hash.len())]
                );
            }
            _ => {
                log::warn!("[cli-install] Could not hash existing CLI — proceeding with copy");
            }
        }
    }

    std::fs::create_dir_all(&dest_dir)?;
    if needs_copy {
        let bytes = std::fs::copy(src, &dest)?;
        let hash_short = file_sha256(&dest)
            .map(|h| h[..12.min(h.len())].to_string())
            .unwrap_or_else(|| "?".into());
        log::info!(
            "[cli-install] Installed CLI ({} bytes, sha256={}) to {}",
            bytes,
            hash_short,
            dest.display()
        );
    }

    let resources = src
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or_else(|| std::io::Error::other("CLI resource root unavailable"))?
        .canonicalize()?;
    let hint = codeterm_types::server_bootstrap::CliResourceHint {
        resources,
        cli_sha256: file_sha256(&dest)
            .ok_or_else(|| std::io::Error::other("Installed CLI digest unavailable"))?,
    };
    codeterm_util::private_file::write_atomic(
        &dest_dir.join(codeterm_types::server_bootstrap::CLI_RESOURCE_HINT_FILE),
        &serde_json::to_vec(&hint).map_err(std::io::Error::other)?,
    )?;

    let alias = dest_dir.join(if cfg!(windows) {
        "domios.exe"
    } else {
        "domios"
    });
    install_cli_alias(&dest, &alias)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if needs_copy {
            let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
        }
        if is_prod_dest {
            // Repair stale ~/.local/bin symlinks (e.g. old launcher) so PATH calls hit this install.
            // Otherwise PATH dispatch can no-op by re-entering the daemon launcher.
            let local_bin = home.join(".local").join("bin");
            let _ = std::fs::create_dir_all(&local_bin);
            for name in ["domios", "codeterm"] {
                let link = local_bin.join(name);
                let needs_create = match std::fs::symlink_metadata(&link) {
                    Ok(meta) if meta.file_type().is_symlink() => match std::fs::read_link(&link) {
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
                    },
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
        }
    }
    if is_prod_dest {
        user_path.add_dir(&dest_dir);
    }

    Ok(needs_copy)
}

fn install_cli_alias(binary: &std::path::Path, alias: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        if std::fs::read_link(alias).is_ok_and(|target| target == binary) {
            return Ok(());
        }
    }
    #[cfg(windows)]
    {
        if alias.exists()
            && file_sha256(binary).is_some_and(|hash| Some(hash) == file_sha256(alias))
        {
            return Ok(());
        }
    }
    match std::fs::remove_file(alias) {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(error),
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(binary, alias)?;
    #[cfg(windows)]
    std::fs::hard_link(binary, alias)?;
    Ok(())
}

/// Install bundled CLI binary to the active data-dir bin/ on startup (runs in background).
pub fn install_bundled_cli(shell: &dyn crate::ports::host::Shell) {
    if !should_install_bundled_cli(std::env::var("CODETERM_DEV").as_deref() == Ok("1")) {
        log::debug!("[cli-install] Dev CLI is owned by build.sh, skipping bundled refresh");
        return;
    }
    let Some(src) = resolve_bundled_cli_src(shell) else {
        log::debug!("[cli-install] No bundled CLI found (dev mode?), skipping");
        return;
    };
    let Some(home) = dirs::home_dir() else {
        log::error!("[cli-install] Cannot determine home directory");
        return;
    };
    let data_dir = crate::paths::data_dir();
    // Run in background to not block app startup.
    std::thread::spawn(move || {
        if let Err(e) = refresh_installed_cli(&src, &home, &data_dir) {
            log::error!("[cli-install] Failed to copy CLI: {}", e);
        }
    });
}

/// Refresh the installed CLI right after an in-app update applies the new
/// bundle, so `~/.domios/bin/codeterm` matches the freshly-installed app's
/// resource CLI without waiting for (or depending on) the next cold startup.
/// Synchronous: the caller is already off the UI hot path (update apply).
pub fn refresh_bundled_cli(shell: &dyn crate::ports::host::Shell) {
    let Some(src) = resolve_bundled_cli_src(shell) else {
        log::debug!("[cli-install] post-update: no bundled CLI in resources, skipping");
        return;
    };
    let Some(home) = dirs::home_dir() else {
        log::error!("[cli-install] post-update: cannot determine home directory");
        return;
    };
    let data_dir = crate::paths::data_dir();
    match refresh_installed_cli(&src, &home, &data_dir) {
        Ok(true) => log::info!("[cli-install] post-update CLI refresh complete"),
        Ok(false) => log::debug!("[cli-install] post-update: CLI already current"),
        Err(e) => log::error!("[cli-install] post-update CLI refresh failed: {}", e),
    }
}

#[cfg(test)]
mod tests;
