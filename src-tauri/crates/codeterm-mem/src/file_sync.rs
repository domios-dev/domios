use crate::{
    backend::{
        engines::{SourceRefresh, TrackedSource},
        MemStore, WriteContext,
    },
    MemManager,
};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::Path, sync::Arc};
const TRACKED_SOURCE_READ_LIMIT: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileSyncResult {
    pub total_tracked: usize,
    pub unchanged: usize,
    pub re_indexed: usize,
    pub source_missing: usize,
    pub source_recovered: usize,
    pub errors: Vec<String>,
}

pub fn sync_all(mem: &Arc<MemManager>) -> Result<FileSyncResult, String> {
    sync_sources(mem.store.as_ref(), None, |id, body| {
        mem.queue_embed(id, body)
    })
}
pub fn sync_path(mem: &Arc<MemManager>, path: &str) -> Result<FileSyncResult, String> {
    sync_sources(mem.store.as_ref(), Some(path), |id, body| {
        mem.queue_embed(id, body)
    })
}
pub fn sync_all_sync(store: &dyn MemStore) -> Result<FileSyncResult, String> {
    sync_sources(store, None, |_, _| {})
}
fn sync_sources(
    store: &dyn MemStore,
    path: Option<&str>,
    enqueue: impl Fn(&str, &str),
) -> Result<FileSyncResult, String> {
    let items: Vec<_> = store
        .tracked_sources()?
        .into_iter()
        .filter(|i| path.is_none_or(|p| i.source_path == p))
        .collect();
    if items.is_empty() && path.is_some() {
        return Err(format!(
            "no tracked item with source_path={}",
            path.unwrap_or_default()
        ));
    }
    let mut result = FileSyncResult {
        total_tracked: items.len(),
        ..Default::default()
    };
    for item in &items {
        match sync_one(store, item) {
            Ok((action, body)) => {
                if let Some(body) = body {
                    enqueue(&item.id, &body);
                }
                apply_action(&mut result, action);
            }
            Err(error) => result.errors.push(format!("{}: {error}", item.id)),
        }
    }
    log_summary(&result);
    Ok(result)
}
#[derive(Debug, Clone, Copy)]
enum SyncAction {
    Unchanged,
    ReIndexed,
    Missing,
    Recovered,
}
fn apply_action(result: &mut FileSyncResult, action: SyncAction) {
    match action {
        SyncAction::Unchanged => result.unchanged += 1,
        SyncAction::ReIndexed => result.re_indexed += 1,
        SyncAction::Missing => result.source_missing += 1,
        SyncAction::Recovered => result.source_recovered += 1,
    }
}

fn log_summary(result: &FileSyncResult) {
    log::info!(
        "[file-sync] tracked={} unchanged={} re-indexed={} missing={} recovered={} errors={}",
        result.total_tracked,
        result.unchanged,
        result.re_indexed,
        result.source_missing,
        result.source_recovered,
        result.errors.len(),
    );
}

fn sync_one(
    store: &dyn MemStore,
    item: &TrackedSource,
) -> Result<(SyncAction, Option<String>), String> {
    let path = Path::new(
        item.source_path
            .strip_prefix("file:")
            .unwrap_or(&item.source_path),
    );
    let missing = serde_json::from_str::<Vec<String>>(&item.tags_json)
        .unwrap_or_default()
        .iter()
        .any(|tag| tag == "source-missing");
    let ctx = WriteContext::system("file-sync");
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(_) => {
            if !missing {
                store.refresh_source(
                    &ctx,
                    item,
                    &SourceRefresh {
                        body: None,
                        size: None,
                        mtime: None,
                        missing: true,
                    },
                )?;
            }
            return Ok((SyncAction::Missing, None));
        }
    };
    let size = meta.len() as i64;
    let mtime = meta
        .modified()
        .ok()
        .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
    if item.source_size == Some(size)
        && item
            .source_mtime
            .as_ref()
            .zip(mtime.as_ref())
            .is_some_and(|(a, b)| a == b)
        && !missing
    {
        return Ok((SyncAction::Unchanged, None));
    }
    let body = read_tracked_source(path, meta.len())
        .map_err(|e| format!("read {}: {e}", item.source_path))?;
    if !store.refresh_source(
        &ctx,
        item,
        &SourceRefresh {
            body: Some(&body),
            size: Some(size),
            mtime: mtime.as_deref(),
            missing: false,
        },
    )? {
        return Ok((SyncAction::Unchanged, None));
    }
    Ok((
        if missing {
            SyncAction::Recovered
        } else {
            SyncAction::ReIndexed
        },
        Some(body),
    ))
}
fn read_tracked_source(path: &Path, size: u64) -> Result<String, String> {
    if size > TRACKED_SOURCE_READ_LIMIT {
        return Err(format!(
            "tracked source exceeds {TRACKED_SOURCE_READ_LIMIT} byte limit"
        ));
    }
    let mut content = String::with_capacity(size as usize);
    std::fs::File::open(path)
        .and_then(|file| {
            file.take(TRACKED_SOURCE_READ_LIMIT)
                .read_to_string(&mut content)
        })
        .map_err(|error| error.to_string())?;
    Ok(content)
}

#[cfg(test)]
mod tests;
