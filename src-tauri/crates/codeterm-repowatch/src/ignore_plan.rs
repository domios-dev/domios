use codeterm_util::watch_plan::DirectoryPlan;
use git2::Repository;
use std::path::{Path, PathBuf};

pub(crate) fn plan(root: &Path) -> Result<DirectoryPlan, String> {
    let repo = Repository::open(root).map_err(|error| error.to_string())?;
    let tracked = crate::tracked::paths(&repo)?;
    let config = repo.config().map_err(|error| error.to_string())?;
    let global = match config.get_path("core.excludesFile") {
        Ok(path) => Some(if path.is_absolute() {
            path
        } else {
            root.join(path)
        }),
        Err(error) if error.code() == git2::ErrorCode::NotFound => default_global(),
        Err(error) => return Err(error.to_string()),
    };
    let mut sources: Vec<_> = global.into_iter().collect();
    sources.push(repo.commondir().join("info/exclude"));
    codeterm_util::watch_plan::directory_plan(root, &tracked, &sources)
}

fn default_global() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|home| PathBuf::from(home).join(".config"))
        })
        .map(|directory| directory.join("git/ignore"))
}

#[cfg(test)]
mod tests;
