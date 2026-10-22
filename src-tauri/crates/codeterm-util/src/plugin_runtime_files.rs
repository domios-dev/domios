use std::{fs, io, path::Path};

pub fn runtime_file_rules(root: &Path) -> io::Result<Option<Vec<String>>> {
    match fs::read_to_string(root.join("runtime-files.txt")) {
        Ok(text) => Ok(Some(
            text.lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn runtime_file_allowed(relative: &str, rules: &[String]) -> bool {
    if relative.split('/').any(|part| {
        part.starts_with('.')
            || matches!(
                part,
                "src" | "tests" | "fixtures" | "node_modules" | "target"
            )
    }) || relative.contains(".test.")
        || relative.ends_with(".ts")
        || relative.ends_with(".tsx")
        || relative.ends_with(".map")
    {
        return false;
    }
    rules
        .iter()
        .any(|rule| relative == rule || (rule.ends_with('/') && relative.starts_with(rule)))
}

pub fn copy_runtime_bundle(source: &Path, destination: &Path) -> io::Result<()> {
    let rules =
        runtime_file_rules(source)?.ok_or_else(|| io::Error::other("missing runtime-files.txt"))?;
    fs::create_dir_all(destination)?;
    copy_directory(source, source, destination, &rules)
}

fn copy_directory(
    root: &Path,
    source: &Path,
    destination: &Path,
    rules: &[String],
) -> io::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.')
            || matches!(
                name.as_ref(),
                "node_modules" | "target" | "Thumbs.db" | "desktop.ini"
            )
        {
            continue;
        }
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(io::Error::other(format!(
                "symlink not allowed in plugin bundle: {}",
                path.display()
            )));
        }
        if kind.is_dir() {
            copy_directory(root, &path, destination, rules)?;
        } else if kind.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(io::Error::other)?
                .to_string_lossy()
                .replace('\\', "/");
            if runtime_file_allowed(&relative, rules) {
                let target = destination.join(relative);
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(path, target)?;
            }
        }
    }
    Ok(())
}
