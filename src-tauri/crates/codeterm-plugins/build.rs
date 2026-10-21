#[path = "../codeterm-util/src/bundled_plugin_dirs.rs"]
mod bundled_plugin_dirs;

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins");
    let mut plugins = Vec::new();
    for entry in std::fs::read_dir(&root).expect("read bundled plugins") {
        let entry = entry.expect("read plugin entry");
        if !entry.file_type().expect("plugin type").is_dir() {
            continue;
        }
        let directory = entry.file_name().to_string_lossy().into_owned();
        let Some(id) = bundled_plugin_dirs::bundled_plugin_id(&directory) else {
            continue;
        };
        let manifest_path = entry.path().join("plugin.json");
        // provider-shared holds shared plugin sources, not a plugin; only a manifest makes one.
        if !manifest_path.is_file() {
            continue;
        }
        let manifest = std::fs::read_to_string(&manifest_path).expect("bundled plugin manifest");
        println!("cargo:rerun-if-changed={}", manifest_path.display());
        plugins.push((id.to_owned(), directory, manifest));
    }
    plugins.sort_by(|a, b| a.0.cmp(&b.0));
    let mut table = format!(
        "pub(super) const SYSTEM_PLUGIN_IDS: [&str; {}] = [",
        plugins.len()
    );
    for (id, _, _) in &plugins {
        table.push_str(&format!("{id:?},"));
    }
    table.push_str("];\npub(super) const BUNDLED_PLUGINS: &[(&str, &str, &str)] = &[\n");
    for (id, directory, manifest) in plugins {
        table.push_str(&format!("({id:?}, {directory:?}, {manifest:?}),\n"));
    }
    table.push_str("];\n");
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(output.join("bundled_plugins.rs"), table).expect("write bundled metadata");
    println!("cargo:rerun-if-changed={}", root.display());
    println!("cargo:rerun-if-changed=../codeterm-util/src/bundled_plugin_dirs.rs");
}
