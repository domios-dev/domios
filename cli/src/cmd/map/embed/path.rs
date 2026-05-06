use std::path::{Path, PathBuf};

pub fn resolve_embed_path() -> Result<PathBuf, String> {
    let exe_name = if cfg!(windows) { "codeterm-embed.exe" } else { "codeterm-embed" };
    let triplet_names = arch_triplet_names();

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
        let parents: Vec<&Path> = current.ancestors().take(6).collect();
        for dir in parents {
            for c in candidates_in(dir, exe_name, &triplet_names) {
                if c.is_file() {
                    return Ok(c);
                }
            }
        }
    }

    for root in workspace_search_roots() {
        for c in candidates_in(&root, exe_name, &triplet_names) {
            if c.is_file() {
                return Ok(c);
            }
        }
    }

    if let Some(found) = which_on_path(exe_name) {
        return Ok(found);
    }

    Err(format!("codeterm-embed not found (set CODETERM_EMBED_PATH to override)"))
}

fn candidates_in(dir: &Path, exe_name: &str, triplets: &[String]) -> Vec<PathBuf> {
    let mut out = vec![
        dir.join(exe_name),
        dir.join("bundled-embed").join(exe_name),
        dir.join("src-tauri").join("bundled-embed").join(exe_name),
    ];
    for t in triplets {
        out.push(dir.join("bundled-embed").join(t));
        out.push(dir.join("src-tauri").join("bundled-embed").join(t));
    }
    out
}

fn arch_triplet_names() -> Vec<String> {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    let mut names = Vec::new();
    let triplet = match (arch, os) {
        ("aarch64", "macos") => Some("codeterm-embed-aarch64-apple-darwin"),
        ("x86_64", "macos") => Some("codeterm-embed-x86_64-apple-darwin"),
        ("x86_64", "linux") => Some("codeterm-embed-x86_64-unknown-linux-gnu"),
        ("aarch64", "linux") => Some("codeterm-embed-aarch64-unknown-linux-gnu"),
        ("x86_64", "windows") => Some("codeterm-embed-x86_64-pc-windows-msvc.exe"),
        _ => None,
    };
    if let Some(t) = triplet {
        names.push(t.to_string());
    }
    names
}

fn workspace_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        for anc in cwd.ancestors().take(8) {
            roots.push(anc.to_path_buf());
        }
    }
    roots
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
