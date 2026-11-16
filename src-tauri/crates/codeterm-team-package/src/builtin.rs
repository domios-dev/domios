use std::path::Path;

use include_dir::{include_dir, Dir};

use crate::{check_entries, CheckOptions, Checked, PackageFormat};

/// Every builtin package in folder form: `<package>/team.yaml` plus its referenced files.
static TEMPLATES: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../../team-templates");

const ROOT_DOCUMENT: &str = "team.yaml";

fn portable(path: &Path) -> String {
    path.components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>()
        .join("/")
}

fn collect(dir: &Dir<'static>, root: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    for file in dir.files() {
        let relative = file.path().strip_prefix(root).unwrap_or(file.path());
        out.push((portable(relative), file.contents().to_vec()));
    }
    for child in dir.dirs() {
        collect(child, root, out);
    }
}

fn is_package(dir: &Dir<'static>) -> bool {
    dir.files()
        .any(|file| file.path().file_name().and_then(|name| name.to_str()) == Some(ROOT_DOCUMENT))
}

fn folder_name(dir: &Dir<'static>) -> String {
    dir.path()
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

/// The builtin package folders, indexed by their root `team.yaml`; adding one needs no registry edit.
pub fn names() -> Vec<String> {
    let mut names: Vec<String> = TEMPLATES
        .dirs()
        .filter(|dir| is_package(dir))
        .map(folder_name)
        .collect();
    names.sort();
    names
}

/// One builtin package's entries, sorted by path.
pub fn entries(name: &str) -> Option<Vec<(String, Vec<u8>)>> {
    let dir = TEMPLATES
        .dirs()
        .find(|dir| is_package(dir) && folder_name(dir) == name)?;
    let mut out = Vec::new();
    collect(dir, dir.path(), &mut out);
    out.sort_by(|left, right| left.0.cmp(&right.0));
    Some(out)
}

/// Validates one builtin package through the same rules as any folder package.
pub fn check(name: &str, options: &CheckOptions) -> Option<Checked> {
    entries(name).map(|entries| check_entries(PackageFormat::Folder, entries, Vec::new(), options))
}
