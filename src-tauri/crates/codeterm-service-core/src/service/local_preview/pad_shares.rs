use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use codeterm_local_preview::artifact_directive::{canvas_file_paths, scan_artifacts, FileCard};
use codeterm_local_preview::html_preview::{
    checked_path, is_canvas_file, open_pad_share_source, LocalPreviewFile, CANVAS_FILE_LIMIT,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::{agent_pad_root, artifact_policy, read_with, references::path_key};
use crate::service::ServiceContext;

mod attachments;
pub use attachments::PadAttachments;

const MAX_FILES: usize = 32;
const MAX_STORE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Grant {
    sender: String,
    recipient: String,
    pad: PathBuf,
    file: PathBuf,
    items: BTreeSet<PathBuf>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Grants(Vec<Grant>);

impl Grants {
    fn insert(&mut self, grant: Grant) {
        if let Some(existing) = self.0.iter_mut().find(|entry| {
            entry.sender == grant.sender
                && entry.recipient == grant.recipient
                && entry.pad == grant.pad
                && entry.file == grant.file
        }) {
            existing.items.extend(grant.items);
        } else {
            self.0.push(grant);
        }
    }

    fn revoke(&mut self, tab: &str) -> bool {
        let before = self.0.len();
        self.0
            .retain(|grant| grant.sender != tab && grant.recipient != tab);
        self.0.len() != before
    }

    fn find(&self, tab: &str, path: &Path, is_canvas: bool) -> Option<Grant> {
        let key = path_key(path);
        self.0
            .iter()
            .find(|grant| {
                grant.recipient == tab
                    && (path_key(&grant.file) == key
                        || (!is_canvas && grant.items.iter().any(|item| path_key(item) == key)))
            })
            .cloned()
    }
}

#[derive(Default)]
pub(in crate::service) struct Store(Mutex<Option<Grants>>);

fn state_path(svc: &ServiceContext) -> PathBuf {
    svc.agent_pad_data_dir
        .join("runtime")
        .join("pad-preview-shares.json")
}

fn load(path: &Path) -> Result<Grants, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Grants::default()),
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_STORE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_STORE_BYTES {
        return Err("Pad share store exceeds its bound".into());
    }
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

fn save(path: &Path, grants: &Grants) -> Result<(), String> {
    let text = serde_json::to_string(grants).map_err(|error| error.to_string())?;
    if text.len() as u64 > MAX_STORE_BYTES {
        return Err("Pad share store is full".into());
    }
    crate::persisted_state::StateFile::new(path)
        .with_private_permissions()
        .without_backups()
        .save(&text)
}

fn with_store<T>(
    svc: &ServiceContext,
    action: impl FnOnce(&Path, &mut Grants) -> Result<T, String>,
) -> Result<T, String> {
    let path = state_path(svc);
    let mut store = svc.pad_shares.0.lock();
    if store.is_none() {
        *store = Some(load(&path)?);
    }
    action(
        &path,
        store.as_mut().ok_or("Pad share store is unavailable")?,
    )
}

fn distinct_tabs(svc: &ServiceContext, sender: &str, recipient: &str) -> bool {
    let mux = svc.mux.read();
    sender != recipient
        && [sender, recipient].iter().all(|tab| {
            mux.get_tab(tab)
                .is_some_and(|tab| tab.neighbor_authority.is_none())
        })
}

pub(in crate::service) fn revoke(svc: &ServiceContext, tab: &str) {
    if let Err(error) = with_store(svc, |path, grants| {
        if grants.revoke(tab) {
            save(path, grants)?;
        }
        Ok(())
    }) {
        log::warn!("[pad-preview-shares] close could not persist revocation: {error}");
    }
}

fn strictly_inside(pad: &Path, requested: &Path, resolved: &Path) -> bool {
    resolved
        .strip_prefix(pad)
        .is_ok_and(|relative| !relative.as_os_str().is_empty())
        && path_key(requested) == path_key(resolved)
}

fn open_source(
    pad: &Path,
    policy: &codeterm_local_preview::PreviewPolicy,
    path: &str,
    limit: Option<u64>,
) -> Result<(PathBuf, Option<Vec<u8>>), String> {
    let checked = checked_path(path)?;
    let pad_key = path_key(pad);
    if !path_key(&checked)
        .strip_prefix(pad_key.as_str())
        .is_some_and(|rest| rest.starts_with('/'))
    {
        return Err("Pad share source must be inside the sender's pad".into());
    }
    open_pad_share_source(
        path,
        |requested, canonical| {
            if canonical.is_some_and(|resolved| {
                !strictly_inside(pad, requested, resolved)
                    || !policy.allows_pad_file(pad, requested, resolved)
            }) {
                return Err("Pad share source escaped the sender's pad".into());
            }
            Ok(())
        },
        limit,
    )
}

fn admit(
    sender: &str,
    recipient: &str,
    pad: &Path,
    cards: &[FileCard],
    copied_items: usize,
    mut open: impl FnMut(&str, bool) -> Result<(PathBuf, Option<Vec<u8>>), String>,
) -> Result<Vec<Grant>, String> {
    let mut grants = Vec::new();
    let mut count = copied_items + cards.iter().map(|card| card.paths.len()).sum::<usize>();
    if count > MAX_FILES {
        return Err("A message may contain at most 32 file previews including canvas items".into());
    }
    for card in cards {
        for source in &card.paths {
            let canvas = card.kind == "canvas" && is_canvas_file(Path::new(source));
            let Ok((file, bytes)) = open(source, canvas) else {
                continue;
            };
            if !checked_path(source).is_ok_and(|requested| strictly_inside(pad, &requested, &file))
            {
                continue;
            }
            let mut items = BTreeSet::new();
            if let Some(paths) = bytes
                .as_deref()
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .and_then(canvas_file_paths)
            {
                count += paths.len();
                if count > MAX_FILES {
                    return Err(
                        "A message may contain at most 32 file previews including canvas items"
                            .into(),
                    );
                }
                for child in paths {
                    if let Ok((resolved, _)) = open(&child, false) {
                        if checked_path(&child)
                            .is_ok_and(|path| strictly_inside(pad, &path, &resolved))
                        {
                            items.insert(resolved);
                        }
                    }
                }
            }
            grants.push(Grant {
                sender: sender.into(),
                recipient: recipient.into(),
                pad: pad.into(),
                file,
                items,
            });
        }
    }
    Ok(grants)
}

pub(in crate::service) fn prepare(
    svc: &ServiceContext,
    sender: &str,
    recipient: &str,
    body: &str,
    attachments: &PadAttachments,
) -> Result<Option<String>, String> {
    if attachments.is_empty() && !body.contains("<ds-") {
        return Ok(None);
    }
    let unavailable = || {
        if attachments.is_empty() {
            Ok(None)
        } else {
            Err("Pad attachments require two local, non-concierge tabs and a sender pad".into())
        }
    };
    if !distinct_tabs(svc, sender, recipient) {
        return unavailable();
    }
    if !svc.tab_has_agent_identity(recipient) {
        return unavailable();
    }
    let Some(pad) = agent_pad_root(svc, sender) else {
        return unavailable();
    };
    let configured = svc.configuration().api.preview_secret_paths;
    let policy = artifact_policy(svc, Some(sender), Default::default(), &configured)?;
    let attached = attachments::prepare(svc, recipient, &pad, &policy, body, attachments)?;
    let scan = scan_artifacts(attached.body.as_deref().unwrap_or(body), true);
    if scan.overflow {
        return Err("A message may contain at most 32 file previews".into());
    }
    let grants = admit(
        sender,
        recipient,
        &pad,
        &scan.cards,
        attached.copied_items,
        |path, canvas| open_source(&pad, &policy, path, canvas.then_some(CANVAS_FILE_LIMIT)),
    )?;
    attached.write_snapshot()?;
    if !grants.is_empty() {
        with_store(svc, |path, current| {
            if !distinct_tabs(svc, sender, recipient) {
                return Err("Pad share tab closed before handoff".into());
            }
            let mut next = current.clone();
            for grant in grants {
                next.insert(grant);
            }
            save(path, &next)?;
            *current = next;
            Ok(())
        })?;
    }
    Ok(attached.body)
}

pub(super) fn read(
    svc: &ServiceContext,
    tab: &str,
    configured: &[String],
    raw: &str,
    kind: &str,
    canvas: Option<&str>,
) -> Result<Option<LocalPreviewFile>, String> {
    let root = checked_path(canvas.unwrap_or(raw))?;
    let is_canvas = canvas.is_some();
    let grant = match with_store(svc, |_, grants| Ok(grants.find(tab, &root, is_canvas))) {
        Ok(grant) => grant,
        Err(error) => {
            log::debug!("[pad-preview-shares] grant read failed: {error}");
            return Ok(None);
        }
    };
    let Some(grant) = grant.filter(|grant| distinct_tabs(svc, &grant.sender, tab)) else {
        return Ok(None);
    };
    let requested = checked_path(raw)?;
    let file = if let Some(canvas) = canvas {
        let policy = artifact_policy(svc, Some(tab), Default::default(), configured)?
            .with_exact_pad_read(grant.pad.clone(), grant.file.clone());
        let board = read_with(Ok(policy), canvas, "canvas")?;
        let items = canvas_file_paths(&board.data).ok_or("Canvas file is not a valid canvas")?;
        let current = items
            .iter()
            .any(|item| path_key(Path::new(item)) == path_key(&requested));
        grant
            .items
            .iter()
            .find(|item| current && path_key(item) == path_key(&requested))
    } else {
        std::iter::once(&grant.file)
            .chain(&grant.items)
            .find(|file| path_key(file) == path_key(&requested))
    }
    .ok_or("Preview path is private")?;
    let policy = artifact_policy(svc, Some(tab), Default::default(), configured)?
        .with_exact_pad_read(grant.pad.clone(), file.clone());
    let result = read_with(Ok(policy), raw, kind)?;
    if !distinct_tabs(svc, &grant.sender, tab) {
        return Err("Preview path is private".into());
    }
    Ok(Some(result))
}

#[cfg(test)]
mod tests;
