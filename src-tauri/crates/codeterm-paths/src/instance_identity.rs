use std::io;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedInstancePaths {
    data: PathBuf,
    config: PathBuf,
}

impl ResolvedInstancePaths {
    pub fn resolve(data: &Path, config: &Path) -> io::Result<Self> {
        Ok(Self {
            data: resolve_path(data)?,
            config: resolve_path(config)?,
        })
    }

    pub fn current() -> io::Result<Self> {
        let config = crate::config_path().map_err(io::Error::other)?;
        let data = crate::data_dir();
        Self::resolve(&data, &config)
    }

    pub fn data(&self) -> &Path {
        &self.data
    }

    pub fn config(&self) -> &Path {
        &self.config
    }
}

fn resolve_path(path: &Path) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    if !absolute.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Instance path has no absolute filesystem root",
        ));
    }
    let mut components = absolute.components().peekable();
    let mut resolved = PathBuf::new();
    while matches!(
        components.peek(),
        Some(Component::Prefix(_) | Component::RootDir)
    ) {
        if let Some(component) = components.next() {
            resolved.push(component.as_os_str());
        }
    }
    resolved = resolved.canonicalize()?;
    let mut missing = 0usize;
    let mut directory = true;
    for component in components {
        if !directory {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                "Instance path traverses a file",
            ));
        }
        match component {
            Component::ParentDir => {
                if !resolved.pop() {
                    return Err(io::Error::other(
                        "Instance path escapes its filesystem root",
                    ));
                }
                missing = missing.saturating_sub(1);
            }
            Component::CurDir => {}
            component => {
                resolved.push(component.as_os_str());
                if missing > 0 {
                    missing += 1;
                    continue;
                }
                match std::fs::symlink_metadata(&resolved) {
                    Ok(_) => {
                        resolved = resolved.canonicalize()?;
                        directory = resolved.metadata()?.is_dir();
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => missing = 1,
                    Err(error) => return Err(error),
                }
            }
        }
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests;
