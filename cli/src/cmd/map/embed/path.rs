use std::path::PathBuf;

pub fn resolve_embed_path() -> Result<PathBuf, String> {
    let exe_name = if cfg!(windows) { "codeterm-embed.exe" } else { "codeterm-embed" };

    if let Some(env) = std::env::var_os("CODETERM_EMBED_PATH") {
        let p = PathBuf::from(env);
        if p.is_file() {
            return Ok(p);
        }
    }

    let data_bin = data_dir().join("bin").join(exe_name);
    if data_bin.is_file() {
        return Ok(data_bin);
    }

    if let Ok(current) = std::env::current_exe() {
        if let Some(dir) = current.parent() {
            for c in [
                dir.join(exe_name),
                dir.join("codeterm-embed"),
                dir.join("bundled-embed").join(exe_name),
            ] {
                if c.is_file() {
                    return Ok(c);
                }
            }
        }
    }

    if let Some(found) = which_on_path(exe_name) {
        return Ok(found);
    }

    Err(format!("codeterm-embed not found (set CODETERM_EMBED_PATH to override)"))
}

fn data_dir() -> PathBuf {
    let dir_name = if std::env::var("CODETERM_SOCKET_PATH")
        .map(|p| p.contains("codeterm-dev"))
        .unwrap_or(false)
        || std::env::var("CODETERM_API_PORT").as_deref() == Ok("7686")
    {
        ".codeterm-dev"
    } else {
        ".codeterm"
    };
    dirs::home_dir().unwrap_or_default().join(dir_name)
}

fn which_on_path(exe: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(exe);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
