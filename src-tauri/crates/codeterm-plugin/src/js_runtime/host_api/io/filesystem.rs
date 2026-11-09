use std::fs::OpenOptions;
use std::path::Path;

use serde_json::{json, Value};

use super::super::{current_file_lock_call, held_file_locks, HeldFileLock, PLUGIN_READ_FILE_LIMIT};
use super::{operation, Failure, PluginSandbox, Request};

pub(super) fn handles(request: &Request) -> bool {
    matches!(
        request,
        Request::FileExists { .. }
            | Request::IsSymlink { .. }
            | Request::ReadFile { .. }
            | Request::ReadFileHead { .. }
            | Request::ReadFileTail { .. }
            | Request::WriteFile { .. }
            | Request::WriteFileAtomic { .. }
            | Request::LockFile { .. }
            | Request::UnlockFile { .. }
            | Request::RemoveFile { .. }
            | Request::MakeDirs { .. }
            | Request::WriteTempFile { .. }
            | Request::CanonicalPath { .. }
            | Request::ProjectWriteAllowed { .. }
            | Request::ReadDir { .. }
            | Request::ListChildDirs { .. }
            | Request::WalkDir { .. }
            | Request::JsonBooleanRecord { .. }
    )
}

fn check(operation: &operation::Context) -> Result<(), Failure> {
    operation
        .check()
        .map_err(|error| Failure::from(error.to_string()))
}
fn effect(operation: &operation::Context) -> Result<(), Failure> {
    operation
        .begin_effect()
        .map_err(|error| Failure::from(error.to_string()))
}

pub(super) fn execute(
    request: Request,
    sandbox: &PluginSandbox,
    operation: &operation::Context,
) -> Result<Value, Failure> {
    use Request::*;
    check(operation)?;
    match request {
        FileExists { path } => Ok(json!(sandbox.can_read(&path) && Path::new(&path).exists())),
        IsSymlink { path } => Ok(if sandbox.can_read(&path) {
            json!(std::fs::symlink_metadata(&path)
                .ok()
                .map(|metadata| metadata.file_type().is_symlink()))
        } else {
            Value::Null
        }),
        ReadFile { path } => {
            if !sandbox.can_read(&path) {
                return Ok(Value::Null);
            }
            check(operation)?;
            super::super::read_plugin_file(&path)
                .map(|value| json!(value))
                .map_err(Failure::range)
        }
        ReadFileHead { path, max_bytes } => {
            bounded_read(&path, max_bytes, false, sandbox, operation)
        }
        ReadFileTail { path, max_bytes } => {
            bounded_read(&path, max_bytes, true, sandbox, operation)
        }
        CanonicalPath { path } => {
            if !sandbox.can_read(&path) {
                return Ok(Value::Null);
            }
            check(operation)?;
            Ok(json!(std::fs::canonicalize(&path).ok().map(|path| {
                codeterm_paths::strip_verbatim_prefix(&path.to_string_lossy())
            })))
        }
        ProjectWriteAllowed { root } => Ok(json!(sandbox.project_write_allowed(&root))),
        WriteFile { path, contents } => {
            if !sandbox.can_write(&path) {
                return Ok(json!(false));
            }
            effect(operation)?;
            if let Some(parent) = Path::new(&path)
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
            {
                let _ = std::fs::create_dir_all(parent);
            }
            check(operation)?;
            Ok(json!(std::fs::write(&path, contents).is_ok()))
        }
        WriteFileAtomic { path, contents } => {
            if !sandbox.can_write(&path) {
                return Ok(json!(false));
            }
            effect(operation)?;
            Ok(json!(codeterm_util::atomic_write::write_atomic(
                Path::new(&path),
                contents
            )
            .is_ok()))
        }
        WriteTempFile { contents, suffix } => {
            if !codeterm_decision::physical::effects_allowed() {
                return Ok(Value::Null);
            }
            effect(operation)?;
            Ok(json!(codeterm_util::task_files::write(
                &contents,
                suffix.as_deref()
            )))
        }
        RemoveFile { path } => {
            if !sandbox.can_write(&path) {
                return Ok(json!(false));
            }
            effect(operation)?;
            Ok(json!(std::fs::remove_file(path).is_ok()))
        }
        MakeDirs { path } => {
            if !sandbox.can_write(&path) {
                return Ok(json!(false));
            }
            effect(operation)?;
            Ok(json!(std::fs::create_dir_all(path).is_ok()))
        }
        LockFile { path } => {
            let Some(call_id) = current_file_lock_call() else {
                return Ok(Value::Null);
            };
            if !sandbox.can_write(&path) {
                return Ok(Value::Null);
            }
            effect(operation)?;
            let Ok(file) = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(&path)
            else {
                return Ok(Value::Null);
            };
            check(operation)?;
            if file.try_lock().is_err() {
                return Ok(Value::Null);
            }
            let Ok(path) = std::fs::canonicalize(&path) else {
                return Ok(Value::Null);
            };
            let id = uuid::Uuid::new_v4().to_string();
            held_file_locks().lock().insert(
                id.clone(),
                HeldFileLock {
                    call_id,
                    path,
                    file,
                },
            );
            Ok(json!(id))
        }
        UnlockFile { id } => {
            if !codeterm_decision::physical::effects_allowed() {
                return Ok(json!(false));
            }
            let Some(call_id) = current_file_lock_call() else {
                return Ok(json!(false));
            };
            effect(operation)?;
            let file = {
                let mut locks = held_file_locks().lock();
                if !locks.get(&id).is_some_and(|lock| lock.call_id == call_id) {
                    return Ok(json!(false));
                }
                locks.remove(&id)
            };
            Ok(json!(file.is_some_and(|lock| lock.file.unlock().is_ok())))
        }
        ReadDir { path } => directory(&path, 0, false, sandbox, operation),
        ListChildDirs { path } => directory(&path, 0, true, sandbox, operation),
        WalkDir { path, max_depth } => {
            if max_depth > 8 {
                return Err(Failure::range("host I/O directory depth limit".into()));
            }
            directory(&path, max_depth, false, sandbox, operation)
        }
        JsonBooleanRecord { path, keys, grant } => {
            use crate::provider::PreTrustOutcome;
            let outcome = if !sandbox.can_read(&path) || (grant && !sandbox.can_write(&path)) {
                PreTrustOutcome::Skipped("record access denied".into())
            } else if grant {
                effect(operation)?;
                crate::json_boolean_record::grant(Path::new(&path), &keys)
            } else {
                check(operation)?;
                match crate::json_boolean_record::trusted(Path::new(&path), &keys) {
                    Ok(true) => PreTrustOutcome::AlreadyTrusted,
                    Ok(false) => PreTrustOutcome::Skipped("not trusted".into()),
                    Err(reason) => PreTrustOutcome::Skipped(reason),
                }
            };
            serde_json::to_value(outcome).map_err(|error| Failure::from(error.to_string()))
        }
        _ => Err("unsupported filesystem I/O request".into()),
    }
}

fn bounded_read(
    path: &str,
    max_bytes: u64,
    tail: bool,
    sandbox: &PluginSandbox,
    operation: &operation::Context,
) -> Result<Value, Failure> {
    const TAIL_READ_LIMIT: u64 = 4 * 1024 * 1024;
    let limit = if tail {
        TAIL_READ_LIMIT
    } else {
        PLUGIN_READ_FILE_LIMIT
    };
    if max_bytes > limit {
        return Err(Failure::range("host I/O file read size limit".into()));
    }
    if !sandbox.can_read(path) {
        return Ok(Value::Null);
    }
    check(operation)?;
    Ok(json!(super::super::read_file_bounded(
        path, max_bytes, tail
    )))
}

fn directory(
    path: &str,
    max_depth: usize,
    names_only: bool,
    sandbox: &PluginSandbox,
    operation: &operation::Context,
) -> Result<Value, Failure> {
    if !sandbox.can_read(path) {
        return Ok(if names_only { Value::Null } else { json!([]) });
    }
    let mut pending = vec![(std::path::PathBuf::from(path), 0)];
    let mut result = Vec::new();
    let mut bytes = 0;
    while let Some((directory, depth)) = pending.pop() {
        check(operation)?;
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            check(operation)?;
            let path = entry.path();
            if !sandbox.can_read(&path.to_string_lossy()) {
                continue;
            }
            check(operation)?;
            let is_directory = entry.file_type().is_ok_and(|kind| kind.is_dir());
            if names_only && !is_directory {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let value = if names_only {
                json!(name)
            } else {
                super::super::dir_entry_to_json(&path, &name, entry.metadata().ok())
            };
            bytes += value.to_string().len();
            if result.len() >= super::super::WALK_ENTRY_BUDGET || bytes > 8 * 1024 * 1024 {
                return Err(Failure::range("host I/O directory result limit".into()));
            }
            result.push(value);
            if is_directory && depth < max_depth {
                pending.push((path, depth + 1));
            }
        }
    }
    if names_only {
        result.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    }
    Ok(Value::Array(result))
}
