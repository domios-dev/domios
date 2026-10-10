use std::path::Path;
use std::time::Duration;

pub(super) fn unique_recent_session(
    dir: &Path,
    file_extension: &str,
    exclude: &[String],
    max_age: Option<Duration>,
) -> Option<String> {
    if !dir.exists() {
        return None;
    }
    let recency_limit = max_age.unwrap_or_else(|| Duration::from_secs(300));
    let now = std::time::SystemTime::now();
    let dot_ext = format!(".{file_extension}");
    let mut candidates: Vec<(String, std::time::SystemTime)> = Vec::new();

    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(stem) = name.strip_suffix(&dot_ext) else {
            continue;
        };
        // Heuristic: session ids look uuid-ish (>=32 chars with a dash).
        if stem.len() < 32 || !stem.contains('-') {
            continue;
        }
        if exclude.iter().any(|e| e == stem) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if let Ok(modified) = meta.modified() {
            if let Ok(age) = now.duration_since(modified) {
                if age > recency_limit {
                    continue;
                }
            }
        }
        let sort_key = meta.created().or_else(|_| meta.modified()).ok()?;
        candidates.push((stem.to_string(), sort_key));
    }
    match candidates.as_slice() {
        [(id, _)] => Some(id.clone()),
        _ => None,
    }
}
