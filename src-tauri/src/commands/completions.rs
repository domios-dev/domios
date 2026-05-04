use super::DirCompletion;

#[tauri::command]
pub fn list_dir_completions(cwd: String, prefix: String) -> Result<Vec<DirCompletion>, String> {
    let dir = std::path::Path::new(&cwd);
    if !dir.exists() { return Ok(vec![]); }

    let prefix_lower = prefix.to_lowercase();
    let mut results: Vec<DirCompletion> = Vec::new();

    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let ft = entry.file_type().unwrap_or_else(|_| panic!());
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
