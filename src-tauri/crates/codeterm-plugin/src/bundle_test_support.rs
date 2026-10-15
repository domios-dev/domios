use crate::bundled::BundledPlugins;
use include_dir::{Dir, DirEntry, File};
use std::path::Path;

#[path = "../../codeterm-util/src/bundled_plugin_dirs.rs"]
mod bundled_plugin_dirs;

pub(crate) fn fixtures() -> BundledPlugins {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins");
    let mut hashes = Vec::new();
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&root).expect("read plugin fixtures") {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap();
        let Some(id) = bundled_plugin_dirs::bundled_plugin_id(name) else {
            continue;
        };
        if !path.is_dir() {
            continue;
        }
        let id: &'static str = Box::leak(id.to_string().into_boxed_str());
        let hash: &'static str = Box::leak(
            crate::bundle_hash::hash_bundle(&path)
                .unwrap()
                .into_boxed_str(),
        );
        hashes.push((id, hash));
        entries.push(DirEntry::Dir(read_dir(&path, id)));
    }
    hashes.sort_unstable_by_key(|(id, _)| *id);
    assert!(!hashes.is_empty(), "bundled plugin fixtures are required");
    BundledPlugins {
        hashes: Box::leak(hashes.into_boxed_slice()),
        files: Box::leak(Box::new(Dir::new(
            "",
            Box::leak(entries.into_boxed_slice()),
        ))),
    }
}

fn read_dir(source: &Path, relative: &str) -> Dir<'static> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let name = name.to_str().unwrap();
        if name.starts_with('.')
            || ["node_modules", "target", "Thumbs.db", "desktop.ini"].contains(&name)
        {
            continue;
        }
        let path = entry.path();
        let relative: &'static str = Box::leak(format!("{relative}/{name}").into_boxed_str());
        let kind = entry.file_type().unwrap();
        assert!(!kind.is_symlink());
        if kind.is_dir() {
            entries.push(DirEntry::Dir(read_dir(&path, relative)));
        } else if kind.is_file() {
            let contents = Box::leak(std::fs::read(&path).unwrap().into_boxed_slice());
            entries.push(DirEntry::File(File::new(relative, contents)));
        }
    }
    Dir::new(
        Box::leak(relative.to_string().into_boxed_str()),
        Box::leak(entries.into_boxed_slice()),
    )
}
