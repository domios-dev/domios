use std::fs;
use std::path::Path;

pub fn tighten_existing(root: &Path) {
    tighten_files(root);
    for directory in ["mesh", "storage", "mem", "services"] {
        tighten_files(&root.join(directory));
    }
    for directory in ["plugins", "services", "state-backups"] {
        let entries = match fs::read_dir(root.join(directory)) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                log::warn!("[permissions] cannot inspect {directory}: {error}");
                continue;
            }
        };
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            if directory == "state-backups" {
                tighten_files(&entry.path());
            } else {
                for name in ["config.yaml", "grants.yaml", "state.json", "token"] {
                    tighten_one(&entry.path().join(name));
                }
            }
        }
    }
}

pub fn tighten_config_recovery(path: &Path) {
    let file = crate::persisted_state::StateFile::new(path);
    for copy in file.corrupt_copies().into_iter().chain(file.backups()) {
        tighten_one(&copy);
    }
}

fn tighten_files(root: &Path) {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            log::warn!("[permissions] cannot inspect {}: {error}", root.display());
            return;
        }
    };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if matches!(name.as_ref(), ".secrets" | "config.toml") {
            continue;
        }
        let base = name.split(".corrupt-").next().unwrap_or(&name);
        let state = matches!(
            Path::new(base)
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("toml" | "yaml" | "yml" | "json" | "db" | "sqlite" | "bak")
        ) || matches!(base, ".secrets" | ".secrets.lock" | "token")
            || base.ends_with(".db-wal")
            || base.ends_with(".db-shm");
        if state {
            tighten_one(&entry.path());
        }
    }
}

fn tighten_one(path: &Path) {
    if let Err(error) = crate::private_file::restrict_if_present(path) {
        log::warn!("[permissions] cannot protect {}: {error}", path.display());
    }
}

#[cfg(test)]
mod tests;
