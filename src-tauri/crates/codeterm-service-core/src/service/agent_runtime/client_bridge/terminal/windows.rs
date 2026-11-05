use std::{
    collections::BTreeMap,
    fs::File,
    path::{Component, Path, PathBuf},
};

fn native_extension(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("exe") || extension.eq_ignore_ascii_case("com")
    })
}

pub(in super::super) struct Program {
    pub(in super::super) path: PathBuf,
    _parents: Vec<File>,
    _file: File,
}

fn executable(path: &Path, host_parent: bool) -> Result<Program, String> {
    use super::super::fs::windows as fs;
    let path = if host_parent {
        fs::host_disk_path(path)?
    } else {
        fs::disk_path(path)?
    };
    if !native_extension(&path) {
        return Err("Windows shell dispatch denied".into());
    }
    let parents = fs::pinned(path.parent().ok_or("Missing program parent")?)?;
    let parent = parents.last().ok_or("Missing program parent")?;
    let file = fs::pin_program(parent, path.file_name().ok_or("Missing program filename")?)?;
    let path = fs::host_disk_path(&fs::final_path(&file)?)?;
    Ok(Program {
        path,
        _parents: parents,
        _file: file,
    })
}

pub(super) fn validate_agent_path(search: &str) -> Result<(), String> {
    use super::super::fs::windows as fs;
    for directory in std::env::split_paths(search) {
        if directory.as_os_str().is_empty() {
            continue;
        }
        let directory = fs::disk_path(&directory)?;
        let _pins = fs::pinned_if_exists(&directory)?;
    }
    Ok(())
}

pub(in super::super) fn program(
    command: &str,
    env: &BTreeMap<String, String>,
    agent_path: bool,
) -> Result<Program, String> {
    if agent_path {
        let search = env
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
            .map(|(_, value)| value)
            .ok_or("Agent PATH unavailable")?;
        validate_agent_path(search)?;
    }
    let path = Path::new(command);
    if path.is_absolute() {
        return executable(path, false);
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
            if let Ok(program) = executable(&directory.join(name), !agent_path) {
                return Ok(program);
            }
        }
    }
    Err("Native program unavailable in absolute PATH entries".into())
}
