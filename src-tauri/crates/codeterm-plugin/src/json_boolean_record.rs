use std::collections::HashSet;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{Read, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::provider::PreTrustOutcome;

const MAX_ATTEMPTS: usize = 3;
const MAX_BYTES: u64 = 16 * 1024 * 1024;

struct Node {
    span: Range<usize>,
    members: Option<Vec<(String, Node)>>,
}

struct Parser<'a> {
    source: &'a str,
    at: usize,
}

impl Parser<'_> {
    fn whitespace(&mut self) {
        while self
            .source
            .as_bytes()
            .get(self.at)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.at += 1;
        }
    }

    fn string(&mut self) -> Result<String, ()> {
        let start = self.at;
        self.at += 1;
        loop {
            match self.source.as_bytes().get(self.at) {
                Some(b'\\') => self.at += 2,
                Some(b'"') => {
                    self.at += 1;
                    return serde_json::from_str(&self.source[start..self.at]).map_err(|_| ());
                }
                Some(_) => self.at += 1,
                None => return Err(()),
            }
        }
    }

    fn node(&mut self, depth: usize) -> Result<Node, ()> {
        if depth > 128 {
            return Err(());
        }
        self.whitespace();
        let start = self.at;
        let members = match self.source.as_bytes().get(self.at) {
            Some(b'{') => {
                self.at += 1;
                self.whitespace();
                let mut members = Vec::new();
                let mut seen = HashSet::new();
                while self.source.as_bytes().get(self.at) != Some(&b'}') {
                    if self.source.as_bytes().get(self.at) != Some(&b'"') {
                        return Err(());
                    }
                    let key = self.string()?;
                    if !seen.insert(key.clone()) {
                        return Err(());
                    }
                    self.whitespace();
                    if self.source.as_bytes().get(self.at) != Some(&b':') {
                        return Err(());
                    }
                    self.at += 1;
                    members.push((key, self.node(depth + 1)?));
                    self.whitespace();
                    if self.source.as_bytes().get(self.at) == Some(&b',') {
                        self.at += 1;
                        self.whitespace();
                    } else {
                        break;
                    }
                }
                if self.source.as_bytes().get(self.at) != Some(&b'}') {
                    return Err(());
                }
                self.at += 1;
                Some(members)
            }
            Some(b'[') => {
                self.at += 1;
                self.whitespace();
                while self.source.as_bytes().get(self.at) != Some(&b']') {
                    self.node(depth + 1)?;
                    self.whitespace();
                    if self.source.as_bytes().get(self.at) == Some(&b',') {
                        self.at += 1;
                        self.whitespace();
                    } else {
                        break;
                    }
                }
                if self.source.as_bytes().get(self.at) != Some(&b']') {
                    return Err(());
                }
                self.at += 1;
                None
            }
            Some(b'"') => {
                self.string()?;
                None
            }
            Some(_) => {
                while self
                    .source
                    .as_bytes()
                    .get(self.at)
                    .is_some_and(|b| !b.is_ascii_whitespace() && !b",]}".contains(b))
                {
                    self.at += 1;
                }
                if self.at == start {
                    return Err(());
                }
                None
            }
            None => return Err(()),
        };
        Ok(Node {
            span: start..self.at,
            members,
        })
    }
}

pub(crate) enum Edit {
    AlreadyTrue,
    Replace(String),
}

pub(crate) fn plan(source: &str, keys: &[String]) -> Result<Edit, String> {
    serde_json::from_str::<serde_json::Value>(source).map_err(|_| "invalid JSON")?;
    if keys.is_empty() || keys.len() > 16 {
        return Err("invalid key path".into());
    }
    let root = Parser { source, at: 0 }
        .node(0)
        .map_err(|_| "ambiguous JSON")?;
    let mut node = &root;
    for (depth, key) in keys.iter().enumerate() {
        let members = node
            .members
            .as_ref()
            .ok_or("record parent is not an object")?;
        if let Some((_, child)) = members.iter().find(|(name, _)| name == key) {
            node = child;
            continue;
        }
        let mut value = serde_json::Value::Bool(true);
        for remaining in keys[depth + 1..].iter().rev() {
            let mut object = serde_json::Map::new();
            object.insert(remaining.clone(), value);
            value = serde_json::Value::Object(object);
        }
        let indent = "  ".repeat(depth + 1);
        let formatted = serde_json::to_string_pretty(&value)
            .map_err(|_| "cannot encode value")?
            .replace('\n', &format!("\n{indent}"));
        let member = format!(
            "{}: {formatted}",
            serde_json::to_string(key).map_err(|_| "cannot encode key")?
        );
        let (at, insertion) = match members.last() {
            Some((_, last)) => (last.span.end, format!(",\n{indent}{member}")),
            None => (
                node.span.start + 1,
                format!("\n{indent}{member}\n{}", "  ".repeat(depth)),
            ),
        };
        let mut result = source.to_string();
        result.insert_str(at, &insertion);
        return Ok(Edit::Replace(result));
    }
    match &source[node.span.clone()] {
        "true" => Ok(Edit::AlreadyTrue),
        "false" => {
            let mut result = source.to_string();
            result.replace_range(node.span.clone(), "true");
            Ok(Edit::Replace(result))
        }
        _ => Err("record value is not a boolean".into()),
    }
}

fn same_version(a: &Metadata, b: &Metadata) -> bool {
    a.len() == b.len()
        && a.modified().ok().is_some()
        && a.modified().ok() == b.modified().ok()
        && same_identity(a, b)
}

#[cfg(unix)]
fn same_identity(a: &Metadata, b: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev() && a.ino() == b.ino()
}

#[cfg(not(unix))]
fn same_identity(_a: &Metadata, _b: &Metadata) -> bool {
    true
}

fn snapshot(path: &Path) -> Result<(Metadata, String), PreTrustOutcome> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| PreTrustOutcome::Skipped("record unavailable".into()))?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_BYTES {
        return Err(PreTrustOutcome::Skipped(
            "record is not a bounded regular file".into(),
        ));
    }
    let mut file =
        File::open(path).map_err(|_| PreTrustOutcome::Skipped("record unreadable".into()))?;
    let opened = file
        .metadata()
        .map_err(|_| PreTrustOutcome::Failed("record metadata unavailable".into()))?;
    if !same_version(&metadata, &opened) {
        return Err(PreTrustOutcome::Failed("record changed during open".into()));
    }
    let mut source = String::new();
    Read::by_ref(&mut file)
        .take(MAX_BYTES + 1)
        .read_to_string(&mut source)
        .map_err(|_| PreTrustOutcome::Skipped("record unreadable".into()))?;
    if source.len() as u64 > MAX_BYTES {
        return Err(PreTrustOutcome::Skipped("record too large".into()));
    }
    Ok((metadata, source))
}

struct TempFile {
    path: PathBuf,
    owned: bool,
}
impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.owned {
            return;
        }
        #[cfg(windows)]
        if let Ok(metadata) = fs::metadata(&self.path) {
            let mut permissions = metadata.permissions();
            permissions.set_readonly(false);
            let _ = fs::set_permissions(&self.path, permissions);
        }
        let _ = fs::remove_file(&self.path);
    }
}

pub(crate) fn trusted(path: &Path, keys: &[String]) -> Result<bool, String> {
    let (_, source) = snapshot(path).map_err(|_| "record unavailable".to_string())?;
    plan(&source, keys).map(|edit| matches!(edit, Edit::AlreadyTrue))
}

pub(crate) fn grant(path: &Path, keys: &[String]) -> PreTrustOutcome {
    grant_with(path, keys, |_, _| {})
}

fn grant_with(
    path: &Path,
    keys: &[String],
    mut before_verify: impl FnMut(usize, &Path),
) -> PreTrustOutcome {
    for attempt in 0..MAX_ATTEMPTS {
        let (metadata, source) = match snapshot(path) {
            Ok(snapshot) => snapshot,
            Err(outcome) => return outcome,
        };
        let replacement = match plan(&source, keys) {
            Ok(Edit::AlreadyTrue) => return PreTrustOutcome::AlreadyTrusted,
            Ok(Edit::Replace(replacement)) => replacement,
            Err(reason) => return PreTrustOutcome::Skipped(reason),
        };
        let Some(parent) = path.parent() else {
            return PreTrustOutcome::Skipped("record has no parent".into());
        };
        let mut temporary = TempFile {
            path: parent.join(format!(".trust-{}.tmp", uuid::Uuid::new_v4())),
            owned: false,
        };
        let mut prepare = || -> std::io::Result<()> {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
                options.mode(metadata.permissions().mode());
            }
            let mut file = options.open(&temporary.path)?;
            temporary.owned = true;
            file.set_permissions(metadata.permissions())?;
            file.write_all(replacement.as_bytes())?;
            file.flush()?;
            file.sync_all()
        };
        if prepare().is_err() {
            return PreTrustOutcome::Failed("temporary record write failed".into());
        }
        before_verify(attempt, path);
        let current = match snapshot(path) {
            Ok(current) => current,
            Err(outcome) => return outcome,
        };
        if !same_version(&metadata, &current.0) || current.1 != source {
            continue;
        }
        let final_metadata = match fs::symlink_metadata(path) {
            Ok(value) if value.file_type().is_file() => value,
            _ => return PreTrustOutcome::Skipped("record is no longer a regular file".into()),
        };
        if !same_version(&metadata, &final_metadata) {
            continue;
        }
        // A refused replacement (including Windows share-delete denial) is terminal.
        if fs::rename(&temporary.path, path).is_err() {
            return PreTrustOutcome::Failed("atomic record replacement failed".into());
        }
        temporary.owned = false;
        return PreTrustOutcome::Granted;
    }
    PreTrustOutcome::Failed("record kept changing after three attempts".into())
}

#[cfg(test)]
mod tests;
