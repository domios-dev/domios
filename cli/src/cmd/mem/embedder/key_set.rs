use std::io::{IsTerminal, Read};
use std::path::Path;

use codeterm_util::private_file;
use serde_json::{json, Value};
use zeroize::Zeroizing;

pub(crate) fn handle(json_mode: bool, provider: &str) {
    let root = codeterm_paths::data_dir_path_for_instance(
        crate::data_dir_name() != ".domios",
        crate::resolved_instance().value.as_deref(),
    );
    let stdin = std::io::stdin();
    match set(&root, provider, stdin.lock(), stdin.is_terminal()) {
        Ok(value) if json_mode => println!("{value}"),
        Ok(value) => println!(
            "Stored embedder key at {}",
            value["path"].as_str().unwrap_or_default()
        ),
        Err(error) => crate::cmd::print::fail(1, error),
    }
}

fn set(root: &Path, provider: &str, input: impl Read, is_terminal: bool) -> Result<Value, String> {
    if provider.is_empty()
        || provider.len() > 64
        || !provider
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err("provider must contain 1..=64 lowercase letters, digits or hyphens".into());
    }
    if is_terminal {
        return Err(
            "refusing terminal input; pipe the key to mem embedder key set on stdin".into(),
        );
    }
    let mut key = Zeroizing::new(String::new());
    input
        .take(65537)
        .read_to_string(&mut key)
        .map_err(|_| "could not read key from stdin")?;
    if key.trim().is_empty() || key.len() > 65536 {
        return Err("stdin must contain a nonempty key of at most 65536 bytes".into());
    }
    if !root.is_absolute() {
        return Err("the resolved data directory must be absolute".into());
    }
    std::fs::create_dir_all(root).map_err(|error| format!("create data directory: {error}"))?;
    let root = root
        .canonicalize()
        .map_err(|error| format!("resolve data directory: {error}"))?;
    let secrets = root.join("secrets");
    std::fs::create_dir_all(&secrets)
        .map_err(|error| format!("create secrets directory: {error}"))?;
    let secrets = secrets
        .canonicalize()
        .map_err(|error| format!("resolve secrets directory: {error}"))?;
    if !secrets.starts_with(&root) || secrets == root {
        return Err("refusing a secrets directory outside the resolved data directory".into());
    }
    let path = secrets.join(format!("{provider}-embedder.key"));
    private_file::write_atomic(&path, key.trim().as_bytes())
        .map_err(|error| format!("write embedder key file: {error}"))?;
    Ok(json!({"ok": true, "provider": provider, "path": path}))
}

#[cfg(test)]
mod tests;
