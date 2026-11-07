use super::{resolve_binary, AgentRuntimeCommand, MAX_ARCHIVE_BYTES};
use codeterm_plugin::agent_runtime::{install_validation, RuntimeProfileInstall};
use sha2::{Digest, Sha256};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::io::AsyncReadExt;

const MAX_EXECUTABLE: u64 = 128 * 1024 * 1024;

pub(super) fn read_executable(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| "Installed executable unavailable")?
        .take(MAX_EXECUTABLE + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Installed executable unavailable")?;
    if bytes.len() as u64 > MAX_EXECUTABLE {
        return Err("Oversized installed executable".into());
    }
    Ok(bytes)
}

fn command(binary: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(binary);
    crate::process_util::no_window_tokio(&mut command);
    command.env_clear();
    for name in [
        "PATH",
        "HOME",
        "USERPROFILE",
        "SystemRoot",
        "TEMP",
        "TMP",
        "TMPDIR",
        "LOCALAPPDATA",
        "APPDATA",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    command
}

pub(super) async fn python(
    target: &Path,
    pin: &str,
) -> Result<(AgentRuntimeCommand, String), String> {
    install_validation::exact_python(pin).ok_or("An exact Python package version is required")?;
    let uv = resolve_binary("uv")?.ok_or("uv is not installed")?;
    tokio::fs::create_dir_all(target)
        .await
        .map_err(|_| "Install directory unavailable")?;
    let target = target
        .canonicalize()
        .map_err(|_| "Install directory unavailable")?;
    let bin = target.join("bin");
    let mut install = command(&uv);
    install
        .args([
            "tool",
            "install",
            "--no-config",
            "--no-progress",
            "--no-build",
            "--no-python-downloads",
            "--python-preference",
            "only-system",
            "--default-index",
            "https://pypi.org/simple",
            pin,
        ])
        .env("UV_TOOL_DIR", target.join("tools"))
        .env("UV_TOOL_BIN_DIR", &bin)
        .env("UV_CACHE_DIR", target.join("cache"))
        .current_dir(&target)
        .stdout(Stdio::null());
    let status = tokio::time::timeout(Duration::from_secs(120), install.status())
        .await
        .map_err(|_| "Python package install deadline exceeded")?
        .map_err(|_| "Python package installer unavailable")?;
    if !status.success() {
        return Err("Pinned Python package install failed (a system Python and prebuilt wheels are required)".into());
    }
    let mut entries = tokio::fs::read_dir(&bin)
        .await
        .map_err(|_| "Python package has no executable")?;
    let mut executable = None;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|_| "Python executable unavailable")?
    {
        let path = entry.path();
        if cfg!(windows) && path.extension().is_none_or(|extension| extension != "exe") {
            continue;
        }
        if !path.is_file() {
            continue;
        }
        if executable.is_some() {
            return Err(
                "Python package declares multiple executables; use an installed local command"
                    .into(),
            );
        }
        executable = Some(
            path.canonicalize()
                .map_err(|_| "Python executable unavailable")?,
        );
    }
    let executable = executable.ok_or("Python package has no executable")?;
    if !executable.starts_with(&target) {
        return Err("Python executable escapes its installation".into());
    }
    let sha = format!("{:x}", Sha256::digest(read_executable(&executable)?));
    Ok((
        AgentRuntimeCommand {
            binary: executable.to_string_lossy().into_owned(),
            args: vec![],
        },
        sha,
    ))
}

pub(super) fn verify(bytes: &[u8], expected: &str) -> Result<(), String> {
    if bytes.len() > MAX_ARCHIVE_BYTES
        || !install_validation::sha256(expected)
        || !expected.eq_ignore_ascii_case(&format!("{:x}", Sha256::digest(bytes)))
    {
        return Err("Binary distribution checksum mismatch or size limit exceeded".into());
    }
    Ok(())
}

pub(super) async fn binary(
    target: &Path,
    spec: &RuntimeProfileInstall,
) -> Result<(AgentRuntimeCommand, String), String> {
    let url = spec
        .url
        .as_deref()
        .filter(|url| install_validation::https(url))
        .ok_or("Invalid binary URL")?;
    let expected = spec
        .sha256
        .as_deref()
        .filter(|sha| install_validation::sha256(sha))
        .ok_or("Binary sha256 is required")?;
    let executable = spec
        .executable
        .as_deref()
        .filter(|name| install_validation::relative_executable(name))
        .ok_or("Invalid binary executable path")?;
    let redirects = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 5 || !install_validation::https(attempt.url().as_str()) {
            attempt.error("Unsafe binary download redirect")
        } else {
            attempt.follow()
        }
    });
    let client = reqwest::Client::builder()
        .redirect(redirects)
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|_| "Binary downloader unavailable")?;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| "Binary download failed")?
        .error_for_status()
        .map_err(|_| "Binary download failed")?;
    if response
        .content_length()
        .is_some_and(|len| len > MAX_ARCHIVE_BYTES as u64)
    {
        return Err("Oversized binary archive".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Binary download failed")?
    {
        if chunk.len() > MAX_ARCHIVE_BYTES.saturating_sub(bytes.len()) {
            return Err("Oversized binary archive".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    verify(&bytes, expected)?;
    tokio::fs::create_dir_all(target)
        .await
        .map_err(|_| "Install directory unavailable")?;
    let target = target
        .canonicalize()
        .map_err(|_| "Install directory unavailable")?;
    let output = target.join(if cfg!(windows) { "agent.exe" } else { "agent" });
    let archive_url = url::Url::parse(url).map_err(|_| "Invalid binary URL")?;
    if [
        ".zip", ".tar.gz", ".tgz", ".tar.xz", ".tar.bz2", ".tar.zst", ".tar",
    ]
    .iter()
    .any(|suffix| archive_url.path().ends_with(suffix))
    {
        let archive = target.join("download.archive");
        tokio::fs::write(&archive, &bytes)
            .await
            .map_err(|_| "Binary archive could not be saved")?;
        let tar = resolve_binary("tar")?.ok_or("tar is required to read this binary archive")?;
        let mut extract = command(&tar);
        extract
            .arg("-xOf")
            .arg(&archive)
            .arg("--")
            .arg(executable)
            .stdout(Stdio::piped())
            .current_dir(&target);
        let mut child = extract.spawn().map_err(|_| "Archive reader unavailable")?;
        let stdout = child.stdout.take().ok_or("Archive reader unavailable")?;
        bytes.clear();
        let extracted = tokio::time::timeout(Duration::from_secs(30), async {
            stdout
                .take(MAX_EXECUTABLE + 1)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| "Archive reader failed")?;
            if bytes.len() as u64 > MAX_EXECUTABLE {
                return Err("Oversized binary executable");
            }
            let status = child.wait().await.map_err(|_| "Archive reader failed")?;
            if !status.success() || bytes.is_empty() {
                return Err("Declared executable unavailable in archive");
            }
            Ok(())
        })
        .await
        .map_err(|_| "Archive reader deadline exceeded")?;
        extracted?;
    }
    tokio::fs::write(&output, &bytes)
        .await
        .map_err(|_| "Binary could not be saved")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o700))
            .await
            .map_err(|_| "Executable permissions could not be set")?;
    }
    Ok((
        AgentRuntimeCommand {
            binary: output.to_string_lossy().into_owned(),
            args: vec![],
        },
        format!("{:x}", Sha256::digest(&bytes)),
    ))
}

#[cfg(test)]
mod tests;
