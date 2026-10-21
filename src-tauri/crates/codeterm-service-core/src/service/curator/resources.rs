use std::io::Read;
use std::path::{Path, PathBuf};

const RECORD_LIMIT: usize = 64 * 1024;
const QUEUE_LIMIT: usize = 128;

pub(super) fn line() -> Option<String> {
    from_directory(&directory()?)
}

fn directory() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        dirs::data_local_dir().map(|root| root.join("domios-build-gateway"))
    } else if cfg!(target_os = "macos") {
        dirs::home_dir().map(|root| root.join("Library/Application Support/CodeTerm/build-gateway"))
    } else {
        dirs::home_dir().map(|root| root.join(".local/state/codeterm-build-gateway"))
    }
}

fn read_bounded(path: &Path, limit: usize) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64).read_to_end(&mut bytes).ok()?;
    (bytes.len() <= limit).then_some(bytes)
}

fn clean(value: &str, limit: usize) -> String {
    value
        .chars()
        .map(|c| if c.is_control() || c == '|' { ' ' } else { c })
        .take(limit)
        .collect()
}

fn from_directory(root: &Path) -> Option<String> {
    let mut queued = 0;
    for entry in std::fs::read_dir(root.join("queue"))
        .ok()?
        .take(QUEUE_LIMIT + 1)
    {
        if !entry.ok()?.file_type().ok()?.is_dir() {
            return None;
        }
        queued += 1;
    }
    let queue = if queued > QUEUE_LIMIT {
        format!("{QUEUE_LIMIT}+")
    } else {
        queued.to_string()
    };
    let id_path = root.join("lock/job_id");
    let gateway = match std::fs::metadata(&id_path) {
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound && !root.join("lock").exists() =>
        {
            "idle".into()
        }
        Err(_) => return None,
        Ok(_) => {
            let bytes = read_bounded(&id_path, 128)?;
            let id = std::str::from_utf8(&bytes).ok()?.trim();
            if id.is_empty()
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                return None;
            }
            let record: serde_json::Value = serde_json::from_slice(&read_bounded(
                &root.join("jobs").join(format!("{id}.json")),
                RECORD_LIMIT,
            )?)
            .ok()?;
            if record["state"].as_str() != Some("running") {
                return None;
            }
            format!(
                "{}/{}",
                clean(record["owner"].as_str()?, 64),
                clean(record["command"].as_str()?, 120)
            )
        }
    };
    Some(format!(
        "resources: gateway {gateway}, queue {queue} (one shared cargo slot)"
    ))
}

#[cfg(test)]
mod tests;
