use super::{agent_pad_root, tab_authority, TabAuthority};
use crate::service::ServiceContext;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAX_RECEIPT_BYTES: u64 = 16 * 1024;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Authority {
    tab_id: String,
    send_id: String,
    scope: TabAuthority,
}

#[derive(Serialize, Deserialize)]
struct Receipt {
    authority: Authority,
    path: PathBuf,
}

pub(crate) fn capture(svc: &ServiceContext, tab: &str, send: &str) -> Result<Authority, String> {
    Ok(Authority {
        tab_id: tab.into(),
        send_id: send.into(),
        scope: tab_authority(svc, tab)?,
    })
}

fn model_name(path: &Path) -> Option<&str> {
    let name = path.file_name()?.to_str()?;
    let id = name.strip_suffix("-model.jpg")?;
    (id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(name)
}

fn receipt_path(pad: &Path, model: &Path) -> Option<PathBuf> {
    Some(
        pad.parent()?
            .parent()?
            .join("model-preview-authority")
            .join(pad.file_name()?)
            .join(format!("{}.json", model_name(model)?)),
    )
}

fn store(path: &Path, receipt: &Receipt) -> Result<(), String> {
    let directory = path
        .parent()
        .ok_or("Model preview authority directory is missing")?;
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
    serde_json::to_writer(file.as_file_mut(), receipt).map_err(|error| error.to_string())?;
    file.flush().map_err(|error| error.to_string())?;
    file.persist(path).map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn record(
    svc: &ServiceContext,
    authority: &Authority,
    model: &Path,
) -> Result<(), String> {
    if authority.send_id.is_empty()
        || authority.scope.nonce.as_deref().is_none_or(str::is_empty)
        || tab_authority(svc, &authority.tab_id)? != authority.scope
    {
        return Err("Model preview send authority changed".into());
    }
    let pad = agent_pad_root(svc, &authority.tab_id).ok_or("Model preview tab pad is missing")?;
    let canonical = model.canonicalize().map_err(|error| error.to_string())?;
    let inbox = pad
        .join("inbox")
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if canonical.parent() != Some(inbox.as_path()) {
        return Err("Model preview escaped its own tab inbox".into());
    }
    let path = codeterm_paths::simplified_path(&canonical);
    let receipt = receipt_path(&pad, &path).ok_or("Model preview name is invalid")?;
    store(
        &receipt,
        &Receipt {
            authority: authority.clone(),
            path,
        },
    )
}

fn load(path: &Path) -> Option<Receipt> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_RECEIPT_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_RECEIPT_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_RECEIPT_BYTES {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

pub(super) fn lookup(
    svc: &ServiceContext,
    tab: &str,
    path: &str,
    current: &TabAuthority,
) -> Result<bool, String> {
    let Some(pad) = agent_pad_root(svc, tab) else {
        return Ok(false);
    };
    let requested = Path::new(path);
    let Some(receipt_path) = receipt_path(&pad, requested) else {
        return Ok(false);
    };
    let Some(mut receipt) = load(&receipt_path) else {
        return Ok(false);
    };
    if receipt.authority.tab_id != tab
        || receipt.authority.send_id.is_empty()
        || receipt
            .authority
            .scope
            .nonce
            .as_deref()
            .is_none_or(str::is_empty)
        || super::references::path_key(requested) != super::references::path_key(&receipt.path)
    {
        return Ok(false);
    }
    // The first session claim belongs to the same launch nonce; pin it once for subsequent reads.
    if receipt.authority.scope.session.is_none() && current.session.is_some() {
        let mut launch = current.clone();
        launch.session = None;
        if receipt.authority.scope == launch {
            receipt.authority.scope = current.clone();
            store(&receipt_path, &receipt)?;
        }
    }
    Ok(receipt.authority.scope == *current)
}

pub(crate) fn retire(pad: &Path, model: &Path) -> Result<(), String> {
    let Some(receipt) = receipt_path(pad, model) else {
        return Ok(());
    };
    match std::fs::remove_file(receipt) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Prune model preview authority: {error}")),
    }
}

#[cfg(test)]
mod tests;
