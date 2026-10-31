use codeterm_update::headless::{Error, ErrorCode, VerifiedManifest};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::File,
    io::{Read, Write},
    path::{Component, Path},
};

const MAX_UNPACKED: u64 = 4 * 1024 * 1024 * 1024;

fn failure(message: impl ToString) -> Error {
    Error::new(ErrorCode::StageFailed, message)
}

fn payload_path(path: &Path) -> bool {
    let Some(text) = path.to_str() else {
        return false;
    };
    !text.contains('\\')
        && path.components().all(|c| matches!(c, Component::Normal(_)))
        && (matches!(
            text,
            "LICENSE"
                | "NOTICE"
                | "server-component.json"
                | "domios-server/domios-server"
                | "cli/codeterm"
                | "cli/domios"
        ) || text.starts_with("dist/")
            || text.starts_with("skills/"))
}

pub fn stage(offer: &VerifiedManifest, archive: &Path, root: &Path) -> Result<String, Error> {
    let metadata = root.symlink_metadata().map_err(failure)?;
    if !metadata.file_type().is_dir() || std::fs::read_dir(root).map_err(failure)?.next().is_some()
    {
        return Err(failure("Headless staging requires an empty directory"));
    }
    let mut source = File::open(archive).map_err(failure)?;
    offer.verify_archive(&mut source)?;
    use std::io::{Seek, SeekFrom};
    source.seek(SeekFrom::Start(0)).map_err(failure)?;
    unpack(source, root)?;
    let build = validate_bundle(root)?;
    let mut version = File::options()
        .create_new(true)
        .write(true)
        .open(root.join(".domios-version"))
        .map_err(failure)?;
    writeln!(version, "{}", offer.manifest().version).map_err(failure)?;
    version.sync_all().map_err(failure)?;
    Ok(build)
}

fn unpack(source: impl Read, root: &Path) -> Result<(), Error> {
    let decoder = flate2::read::GzDecoder::new(source);
    let mut archive = tar::Archive::new(decoder);
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    for entry in archive.entries().map_err(failure)? {
        let mut entry = entry.map_err(failure)?;
        let path = entry.path().map_err(failure)?.into_owned();
        let mode = entry.header().mode().map_err(failure)?;
        let executable = matches!(
            path.to_str(),
            Some("domios-server/domios-server" | "cli/codeterm" | "cli/domios")
        );
        if !entry.header().entry_type().is_file()
            || !payload_path(&path)
            || !seen.insert(path.clone())
            || seen.len() > 100000
            || !matches!(mode, 0o644 | 0o755)
            || (executable && mode != 0o755)
        {
            return Err(failure(
                "Headless archive contains an unsafe, duplicate or unsupported entry",
            ));
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| failure("Headless archive size overflow"))?;
        if total > MAX_UNPACKED {
            return Err(failure("Headless archive exceeds unpacked size limit"));
        }
        let target = root.join(&path);
        let parent = target
            .parent()
            .ok_or_else(|| failure("Headless archive parent missing"))?;
        std::fs::create_dir_all(parent).map_err(failure)?;
        let mut output = File::options()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(failure)?;
        let copied = std::io::copy(&mut entry, &mut output).map_err(failure)?;
        if copied != entry.size() {
            return Err(failure("Truncated headless payload"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            output
                .set_permissions(std::fs::Permissions::from_mode(mode))
                .map_err(failure)?;
        }
        output.sync_all().map_err(failure)?;
    }
    for path in [
        "domios-server/domios-server",
        "cli/codeterm",
        "cli/domios",
        "dist/index.html",
        "server-component.json",
        "LICENSE",
        "NOTICE",
    ] {
        if !seen.contains(Path::new(path)) {
            return Err(failure(format!("Missing headless payload: {path}")));
        }
    }
    Ok(())
}

fn digest(path: &Path) -> Result<String, Error> {
    let mut source = File::open(path).map_err(failure)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let size = source.read(&mut buffer).map_err(failure)?;
        if size == 0 {
            break;
        }
        hash.update(&buffer[..size]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn validate_bundle(root: &Path) -> Result<String, Error> {
    let mut bytes = Vec::new();
    File::open(root.join("server-component.json"))
        .map_err(failure)?
        .take(8193)
        .read_to_end(&mut bytes)
        .map_err(failure)?;
    if bytes.len() > 8192 {
        return Err(failure("Component manifest exceeds bound"));
    }
    let manifest: codeterm_types::server_bootstrap::ServerComponentManifest =
        serde_json::from_slice(&bytes).map_err(failure)?;
    let value = serde_json::to_value(&manifest).map_err(failure)?;
    if value["client_identity"]["kind"] != "none"
        || manifest.schema != codeterm_types::server_bootstrap::SERVER_COMPONENT_SCHEMA
        || manifest.server_executable != Path::new("domios-server/domios-server")
        || manifest.server_sha256 != digest(&root.join("domios-server/domios-server"))?
        || digest(&root.join("cli/codeterm"))? != digest(&root.join("cli/domios"))?
    {
        return Err(failure(
            "Headless component identity or payload digest mismatch",
        ));
    }
    Ok(manifest.server_build)
}

#[cfg(target_os = "linux")]
pub(super) fn switch_link(active: &Path, target: &Path) -> Result<(), Error> {
    let parent = active
        .parent()
        .ok_or_else(|| Error::new(ErrorCode::SwapFailed, "Active bundle parent missing"))?;
    if !target.is_dir()
        || !active
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(Error::new(
            ErrorCode::SwapFailed,
            "Atomic activation requires a staged directory and existing symlink",
        ));
    }
    let temporary = parent.join(format!(".domios-link-{}", uuid::Uuid::new_v4()));
    std::os::unix::fs::symlink(target, &temporary)
        .map_err(|e| Error::new(ErrorCode::SwapFailed, e))?;
    let result =
        std::fs::rename(&temporary, active).map_err(|e| Error::new(ErrorCode::SwapFailed, e));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests;
