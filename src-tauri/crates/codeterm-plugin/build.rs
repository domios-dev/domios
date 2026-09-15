#[path = "src/bundle_hash.rs"]
mod bundle_hash;

/// Core plugins that ship with the app but are not agent providers.
const CORE_PLUGIN_DIRS: &[&str] = &["analytics"];

fn main() {
    generate_system_plugin_hashes();
}

fn generate_system_plugin_hashes() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // codeterm-plugin is at src-tauri/crates/codeterm-plugin → up 3 = repo root.
    let repo_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .expect("unexpected directory depth");
    let plugins_root = repo_root.join("plugins");

    if !plugins_root.is_dir() {
        write_table(&[]);
        return;
    }

    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR not set"));
    let embed_root = out_dir.join("bundled_plugins");
    let _ = std::fs::remove_dir_all(&embed_root);
    std::fs::create_dir_all(&embed_root).expect("mkdir bundled_plugins");

    let mut entries: Vec<(String, String)> = Vec::new();
    for child in std::fs::read_dir(&plugins_root).expect("read plugins dir") {
        let child = child.expect("read plugins entry");
        let path = child.path();
        if !path.is_dir() {
            continue;
        }
        let dir_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let id = match dir_name.strip_prefix("provider-") {
            Some(provider) => provider.to_string(),
            None if CORE_PLUGIN_DIRS.contains(&dir_name.as_str()) => dir_name.clone(),
            None => continue,
        };
        verify_id_matches_folder(&path, &id);
        let h = bundle_hash::hash_bundle(&path)
            .unwrap_or_else(|e| panic!("hash plugin {}: {e}", path.display()));
        entries.push((id.clone(), h));
        copy_filtered(&path, &embed_root.join(&id))
            .unwrap_or_else(|e| panic!("copy plugin {} into OUT_DIR: {e}", path.display()));
        rerun_if_dir_changed(&path);
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));
    write_table(&entries);
    println!("cargo:rerun-if-changed={}", plugins_root.display());
}

fn copy_filtered(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.')
            || name_str == "node_modules"
            || name_str == "target"
            || name_str == "Thumbs.db"
            || name_str == "desktop.ini"
        {
            continue;
        }
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(std::io::Error::other(format!(
                "symlink not allowed in plugin bundle: {}",
                path.display(),
            )));
        }
        let target = dst.join(&name);
        if file_type.is_dir() {
            copy_filtered(&path, &target)?;
        } else if file_type.is_file() {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

fn verify_id_matches_folder(plugin_dir: &std::path::Path, expected_id: &str) {
    let manifest_path = plugin_dir.join("plugin.json");
    let Ok(text) = std::fs::read_to_string(&manifest_path) else {
        return;
    };
    let needle = "\"id\"";
    let Some(idx) = text.find(needle) else { return };
    let after = &text[idx + needle.len()..];
    let after = after.trim_start_matches(|c: char| c.is_whitespace() || c == ':');
    let Some(rest) = after.strip_prefix('"') else {
        return;
    };
    let Some(end) = rest.find('"') else { return };
    let id = &rest[..end];
    if id != expected_id {
        panic!(
            "plugin folder {} declares id={:?} in plugin.json — must match folder suffix {:?}",
            plugin_dir.display(),
            id,
            expected_id,
        );
    }
}

fn rerun_if_dir_changed(dir: &std::path::Path) {
    println!("cargo:rerun-if-changed={}", dir.display());
    for entry in walkdir(dir) {
        println!("cargo:rerun-if-changed={}", entry.display());
    }
}

fn walkdir(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(read) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in read.flatten() {
        let p = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') || name_str == "node_modules" || name_str == "target" {
            continue;
        }
        if p.is_dir() {
            out.extend(walkdir(&p));
        } else {
            out.push(p);
        }
    }
    out
}

fn write_table(entries: &[(String, String)]) {
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    let dest = std::path::Path::new(&out_dir).join("system_plugin_hashes.rs");
    let mut body = String::new();
    body.push_str("// Auto-generated by build.rs — do not edit.\n");
    body.push_str("pub static SYSTEM_PLUGIN_HASHES: &[(&str, &str)] = &[\n");
    for (id, h) in entries {
        body.push_str(&format!("    ({id:?}, {h:?}),\n"));
    }
    body.push_str("];\n");
    std::fs::write(&dest, body).expect("write system_plugin_hashes.rs");
}
