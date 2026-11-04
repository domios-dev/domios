use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

fn native_extension(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("exe") || extension.eq_ignore_ascii_case("com")
    })
}

fn executable(path: &Path) -> Result<PathBuf, String> {
    let path = super::super::fs::windows::disk_path(path)?;
    if !native_extension(&path) {
        return Err("Windows shell dispatch denied".into());
    }
    let _parents =
        super::super::fs::windows::pinned(path.parent().ok_or("Missing program parent")?)?;
    let _file = super::super::fs::windows::read(
        _parents.last().ok_or("Missing program parent")?,
        Path::new(path.file_name().ok_or("Missing program filename")?),
    )?;
    Ok(path)
}

pub(in super::super) fn program(
    command: &str,
    env: &BTreeMap<String, String>,
) -> Result<PathBuf, String> {
    let path = Path::new(command);
    if path.is_absolute() {
        return executable(path);
    }
    if command.contains(['/', '\\', ':'])
        || !matches!(path.components().next(), Some(Component::Normal(_)))
    {
        return Err("Relative program paths denied".into());
    }
    let names = if path.extension().is_none() {
        vec![path.with_extension("exe"), path.with_extension("com")]
    } else if native_extension(path) {
        vec![path.to_path_buf()]
    } else {
        return Err("Windows shell dispatch denied".into());
    };
    let search = env
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
        .map(|(_, value)| value)
        .ok_or("Program PATH unavailable")?;
    for directory in std::env::split_paths(search) {
        if !directory.is_absolute() {
            continue;
        }
        for name in &names {
            if let Ok(program) = executable(&directory.join(name)) {
                return Ok(program);
            }
        }
    }
    Err("Native program unavailable in absolute PATH entries".into())
}
