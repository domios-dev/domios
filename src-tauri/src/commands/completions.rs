use super::DirCompletion;

#[tauri::command]
pub fn list_dir_completions(cwd: String, prefix: String) -> Result<Vec<DirCompletion>, String> {
    let dir = std::path::Path::new(&cwd);
    if !dir.exists() { return Ok(vec![]); }

    let prefix_lower = prefix.to_lowercase();
    let mut results: Vec<DirCompletion> = Vec::new();

    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue; };
        if !ft.is_dir() { continue; }

        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') { continue; }

        let name_lower = name.to_lowercase();

        let score = if name_lower.starts_with(&prefix_lower) {
            0
        } else if name_lower.contains(&prefix_lower) {
            1
        } else if subsequence_match(&name_lower, &prefix_lower) {
            2
        } else {
            continue;
        };

        results.push(DirCompletion { name, score });
    }

    results.sort_by(|a, b| a.score.cmp(&b.score).then(a.name.len().cmp(&b.name.len())));
    results.truncate(20);
    Ok(results)
}

fn subsequence_match(text: &str, query: &str) -> bool {
    let mut qi = query.chars().peekable();
    for tc in text.chars() {
        if qi.peek() == Some(&tc) {
            qi.next();
        }
        if qi.peek().is_none() { return true; }
    }
    qi.peek().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_dir_completions_with_inaccessible_entry_does_not_panic() {
        let result = list_dir_completions(
            std::env::temp_dir().to_string_lossy().to_string(),
            String::new(),
        );
        assert!(result.is_ok(), "should return Ok, not panic");
    }
}
