// Reuse the runtime hash function — single source of truth, no risk
// of build-time vs runtime drift.
#[path = "src/plugin/bundle_hash.rs"]
mod bundle_hash;

fn main() {
    let hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=BUILD_HASH={}", hash.trim());

    generate_system_plugin_hashes();

    // Embed the Common Controls v6 manifest into EVERY target on
    // Windows (bins + test runners). tauri-build only embeds it into
    // the main bin via tauri-winres, which leaves `cargo test --lib`
    // exes without it — and without the manifest, comctl32.dll v6
    // entry points (TaskDialogIndirect, etc) aren't resolved at load
    // time and the test binary fails with STATUS_ENTRYPOINT_NOT_FOUND
    // (0xc0000139). See tauri-apps/tauri#14580.
    //
    // Two pieces:
    //  1. Tell tauri-build NOT to embed its own app manifest (would
    //     conflict with mt.exe on merge → c1010070 parse error).
    //  2. Tell linker to embed ours via /MANIFEST:EMBED + /MANIFESTINPUT.
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=windows-app-manifest.xml");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("windows-app-manifest.xml");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }

    #[cfg(windows)]
    {
        let attrs = tauri_build::Attributes::new().windows_attributes(
            tauri_build::WindowsAttributes::new_without_app_manifest(),
        );
        tauri_build::try_build(attrs).expect("failed to run tauri-build");
    }
    #[cfg(not(windows))]
    {
        tauri_build::build();
    }
}

/// Walk `<repo>/plugins/provider-*/`, hash each bundle canonically, and
/// emit `$OUT_DIR/system_plugin_hashes.rs` with `SYSTEM_PLUGIN_HASHES`.
/// The runtime loader includes that file via `include!`. Failure here
/// fails the build — the hash table is load-bearing for the system-
/// plugin trust model.
fn generate_system_plugin_hashes() {
    use std::path::Path;

    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // src-tauri/.. = repo root.
    let plugins_root = manifest_dir
        .parent()
        .expect("CARGO_MANIFEST_DIR has no parent")
        .join("plugins");

    if !plugins_root.is_dir() {
        // Cargo publish / packaged source without plugins/ — emit an
        // empty table so the binary still links.
        write_table(&[]);
        return;
    }

    let mut entries: Vec<(String, String)> = Vec::new();
    for child in std::fs::read_dir(&plugins_root)
        .expect("read plugins dir")
    {
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
        if !dir_name.starts_with("provider-") {
            continue;
        }
        let id = dir_name.trim_start_matches("provider-").to_string();

        // Sanity check: plugin.json#id must match folder name. Catches
        // copy-paste mistakes at build time.
        verify_id_matches_folder(&path, &id);

        let h = bundle_hash::hash_bundle(&path)
            .unwrap_or_else(|e| panic!("hash plugin {}: {e}", path.display()));
        entries.push((id, h));

        rerun_if_dir_changed(&path);
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));
    write_table(&entries);
    println!("cargo:rerun-if-changed={}", plugins_root.display());
}

fn verify_id_matches_folder(plugin_dir: &std::path::Path, expected_id: &str) {
    let manifest_path = plugin_dir.join("plugin.json");
    let Ok(text) = std::fs::read_to_string(&manifest_path) else {
        // No manifest yet — let the runtime loader complain. build.rs
        // shouldn't fail just because a partially-scaffolded plugin
        // dir exists.
        return;
    };
    // Light parse — avoid pulling in serde_json as a build-dep just
    // for this one check.
    let needle = "\"id\"";
    let Some(idx) = text.find(needle) else { return };
    let after = &text[idx + needle.len()..];
    let after = after.trim_start_matches(|c: char| c.is_whitespace() || c == ':');
    let Some(rest) = after.strip_prefix('"') else { return };
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
    let Ok(read) = std::fs::read_dir(dir) else { return out };
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
