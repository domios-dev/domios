use crate::fixtures::repo_dir;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;

fn git(args: &[&str]) -> Result<(bool, String), String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_dir())
        .args(args)
        .output()
        .map_err(|error| format!("git: {error}"))?;
    Ok((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
    ))
}

fn relative(path: &Path) -> String {
    path.strip_prefix(repo_dir())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn head() -> Result<String, String> {
    Ok(git(&["rev-parse", "HEAD"])?.1)
}

pub fn last_commit(path: &Path) -> Result<Option<String>, String> {
    let (_, out) = git(&["log", "-1", "--format=%H", "--", &relative(path)])?;
    Ok((!out.is_empty()).then_some(out))
}

pub fn committed_unmodified(path: &Path) -> Result<(), String> {
    let name = relative(path);
    if !git(&["ls-files", "--error-unmatch", &name])?.0 {
        return Err(format!("refused: {name} is not committed"));
    }
    let (clean, _) = git(&["diff", "--quiet", "HEAD", "--", &name])?;
    if !clean {
        return Err(format!(
            "refused: {name} differs from its committed version"
        ));
    }
    Ok(())
}

pub fn never_produced(path: &Path) -> Result<(), String> {
    let name = relative(path);
    if path.exists() {
        return Err(format!(
            "refused: {name} exists; this set was already confirmed"
        ));
    }
    let (_, history) = git(&["log", "--all", "--format=%H", "--", &name])?;
    if !history.is_empty() {
        return Err(format!(
            "refused: {name} is in git history; this set was already confirmed"
        ));
    }
    Ok(())
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub fn manifest_matches(queries: &Path, manifest: &Value, set: &str) -> Result<String, String> {
    let hash = sha256_file(queries)?;
    let recorded = manifest["sets"][set]["sha256"].as_str().unwrap_or_default();
    if recorded.trim_start_matches("sha256:") == hash {
        Ok(hash)
    } else {
        Err(format!(
            "refused: sha256 {hash} of {} does not match manifest sets.{set}.sha256 {recorded:?}",
            queries.display()
        ))
    }
}

pub fn starved(manifest: &Value, set: &str) -> Vec<String> {
    let mut out = manifest["sets"][set]["starved"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            item.as_str()
                .map(str::to_owned)
                .or_else(|| item["original"].as_str().map(str::to_owned))
        })
        .collect::<Vec<_>>();
    out.sort();
    out.dedup();
    out
}
