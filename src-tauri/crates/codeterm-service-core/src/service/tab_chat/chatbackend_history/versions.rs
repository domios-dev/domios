use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};

use super::{removed_id, ChatMsg, META_PREFIX, REMOVED_PREFIX};

const VERSION_PREFIX: &str = "{\"_version\":";
const MAX_RECORD_BYTES: u64 = codeterm_plugin::jsonl_window::MAX_RECORD_BYTES;

#[derive(serde::Serialize, serde::Deserialize)]
struct Version {
    id: String,
    base: u64,
    edits: Vec<Edit>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
enum Key {
    Field(String),
    Index(usize),
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Edit {
    Set {
        path: Vec<Key>,
        value: serde_json::Value,
    },
    Remove {
        path: Vec<Key>,
    },
    Splice {
        path: Vec<Key>,
        start: usize,
        end: usize,
        insert: String,
    },
    Truncate {
        path: Vec<Key>,
        length: usize,
    },
    Append {
        path: Vec<Key>,
        values: Vec<serde_json::Value>,
    },
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Envelope {
    _version: Version,
}

#[derive(serde::Deserialize)]
struct RecordId {
    id: String,
}

pub(super) fn message_id(line: &str) -> io::Result<String> {
    if line.starts_with(VERSION_PREFIX) {
        Ok(serde_json::from_str::<Envelope>(line)
            .map_err(io::Error::other)?
            ._version
            .id)
    } else {
        Ok(serde_json::from_str::<RecordId>(line)
            .map_err(io::Error::other)?
            .id)
    }
}

pub(super) fn base(line: &str) -> io::Result<Option<u64>> {
    if !line.starts_with(VERSION_PREFIX) {
        return Ok(None);
    }
    serde_json::from_str::<Envelope>(line)
        .map(|record| Some(record._version.base))
        .map_err(io::Error::other)
}

pub(super) fn delta(id: &str, base: u64, before: &str, after: &str) -> io::Result<String> {
    let before = serde_json::from_str(before).map_err(io::Error::other)?;
    let after = serde_json::from_str(after).map_err(io::Error::other)?;
    let mut edits = Vec::new();
    diff(&before, &after, &mut Vec::new(), &mut edits);
    serde_json::to_string(&Envelope {
        _version: Version {
            id: id.into(),
            base,
            edits,
        },
    })
    .map_err(io::Error::other)
}

fn diff(
    before: &serde_json::Value,
    after: &serde_json::Value,
    path: &mut Vec<Key>,
    edits: &mut Vec<Edit>,
) {
    use serde_json::Value;
    if before == after {
        return;
    }
    match (before, after) {
        (Value::Object(before), Value::Object(after)) => {
            for (key, value) in after {
                path.push(Key::Field(key.clone()));
                if let Some(previous) = before.get(key) {
                    diff(previous, value, path, edits);
                } else {
                    edits.push(Edit::Set {
                        path: path.clone(),
                        value: value.clone(),
                    });
                }
                path.pop();
            }
            for key in before.keys().filter(|key| !after.contains_key(*key)) {
                path.push(Key::Field(key.clone()));
                edits.push(Edit::Remove { path: path.clone() });
                path.pop();
            }
        }
        (Value::Array(before), Value::Array(after)) => {
            for (index, (previous, value)) in before.iter().zip(after).enumerate() {
                path.push(Key::Index(index));
                diff(previous, value, path, edits);
                path.pop();
            }
            if after.len() < before.len() {
                edits.push(Edit::Truncate {
                    path: path.clone(),
                    length: after.len(),
                });
            } else if after.len() > before.len() {
                edits.push(Edit::Append {
                    path: path.clone(),
                    values: after[before.len()..].to_vec(),
                });
            }
        }
        (Value::String(before), Value::String(after)) => {
            let mut start = before
                .bytes()
                .zip(after.bytes())
                .take_while(|(a, b)| a == b)
                .count();
            while !before.is_char_boundary(start) || !after.is_char_boundary(start) {
                start -= 1;
            }
            let mut suffix = before.as_bytes()[start..]
                .iter()
                .rev()
                .zip(after.as_bytes()[start..].iter().rev())
                .take_while(|(a, b)| a == b)
                .count();
            while !before.is_char_boundary(before.len() - suffix)
                || !after.is_char_boundary(after.len() - suffix)
            {
                suffix -= 1;
            }
            edits.push(Edit::Splice {
                path: path.clone(),
                start,
                end: before.len() - suffix,
                insert: after[start..after.len() - suffix].into(),
            });
        }
        _ => edits.push(Edit::Set {
            path: path.clone(),
            value: after.clone(),
        }),
    }
}

fn target<'a>(
    mut value: &'a mut serde_json::Value,
    path: &[Key],
) -> io::Result<&'a mut serde_json::Value> {
    for key in path {
        value = match key {
            Key::Field(key) => value.get_mut(key),
            Key::Index(index) => value.get_mut(*index),
        }
        .ok_or_else(|| io::Error::other("invalid history version path"))?;
    }
    Ok(value)
}

fn apply(value: &mut serde_json::Value, edit: Edit) -> io::Result<()> {
    let invalid = || io::Error::other("invalid history version edit");
    match edit {
        Edit::Set {
            path,
            value: replacement,
        } => {
            if let Some((last, parent)) = path.split_last() {
                let parent = target(value, parent)?;
                match last {
                    Key::Field(key) => {
                        parent
                            .as_object_mut()
                            .ok_or_else(invalid)?
                            .insert(key.clone(), replacement);
                    }
                    Key::Index(index) => {
                        *parent.get_mut(*index).ok_or_else(invalid)? = replacement;
                    }
                }
            } else {
                *value = replacement;
            }
        }
        Edit::Remove { path } => {
            let (Key::Field(key), parent) = path.split_last().ok_or_else(invalid)? else {
                return Err(invalid());
            };
            target(value, parent)?
                .as_object_mut()
                .ok_or_else(invalid)?
                .remove(key);
        }
        Edit::Splice {
            path,
            start,
            end,
            insert,
        } => {
            let serde_json::Value::String(text) = target(value, &path)? else {
                return Err(invalid());
            };
            if start > end || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
                return Err(invalid());
            }
            text.replace_range(start..end, &insert);
        }
        Edit::Truncate { path, length } => {
            target(value, &path)?
                .as_array_mut()
                .ok_or_else(invalid)?
                .truncate(length);
        }
        Edit::Append { path, values } => {
            target(value, &path)?
                .as_array_mut()
                .ok_or_else(invalid)?
                .extend(values);
        }
    }
    Ok(())
}

fn record(file: &mut std::fs::File, at: u64) -> io::Result<String> {
    file.seek(SeekFrom::Start(at))?;
    let mut bytes = Vec::new();
    BufReader::new(file.take(MAX_RECORD_BYTES)).read_until(b'\n', &mut bytes)?;
    if bytes.pop() != Some(b'\n') {
        return Err(io::Error::other("uncommitted history record"));
    }
    String::from_utf8(bytes).map_err(io::Error::other)
}

pub(super) fn resolve(file: &mut std::fs::File, mut at: u64, id: &str) -> io::Result<String> {
    let mut pending = Vec::new();
    let mut scanned = 0;
    let full = loop {
        let line = record(file, at)?;
        scanned += line.len() as u64;
        if scanned > 2 * MAX_RECORD_BYTES {
            return Err(io::Error::other("history version chain exceeds byte limit"));
        }
        if message_id(&line)? != id {
            return Err(io::Error::other("history version identity mismatch"));
        }
        if !line.starts_with(VERSION_PREFIX) {
            break line;
        }
        let version = serde_json::from_str::<Envelope>(&line)
            .map_err(io::Error::other)?
            ._version;
        if version.base >= at {
            return Err(io::Error::other(
                "history version must refer to an earlier record",
            ));
        }
        at = version.base;
        pending.push(version);
    };
    let mut value: serde_json::Value = serde_json::from_str(&full).map_err(io::Error::other)?;
    for version in pending.into_iter().rev() {
        for edit in version.edits {
            apply(&mut value, edit)?;
        }
    }
    let message: ChatMsg = serde_json::from_value(value.clone()).map_err(io::Error::other)?;
    if message.id != id {
        return Err(io::Error::other("history version changed identity"));
    }
    serde_json::to_string(&value).map_err(io::Error::other)
}

pub(super) fn fold(
    file: &mut std::fs::File,
    bytes: &[u8],
    base: u64,
) -> io::Result<(Vec<ChatMsg>, Vec<u64>)> {
    let mut order = Vec::<(String, u64)>::new();
    let mut latest = HashMap::new();
    let mut offset = base;
    for raw in bytes.split_inclusive(|byte| *byte == b'\n') {
        let start = offset;
        offset += raw.len() as u64;
        if raw.last() != Some(&b'\n') {
            break;
        }
        let Ok(line) = std::str::from_utf8(raw) else {
            continue;
        };
        let line = line.trim();
        if line.is_empty() || line.starts_with(META_PREFIX) {
            continue;
        }
        if line.starts_with(REMOVED_PREFIX) {
            if let Some(id) = removed_id(line) {
                latest.remove(&id);
                order.retain(|(kept, _)| *kept != id);
            }
        } else if let Ok(id) = message_id(line) {
            if !latest.contains_key(&id) {
                order.push((id.clone(), start));
            }
            latest.insert(id, start);
        }
    }
    order
        .into_iter()
        .map(|(id, first)| {
            let at = latest[&id];
            let line = resolve(file, at, &id)?;
            let message = serde_json::from_str::<ChatMsg>(&line).map_err(io::Error::other)?;
            Ok((message.with_segments(), first))
        })
        .collect::<io::Result<Vec<_>>>()
        .map(|rows| rows.into_iter().unzip())
}
