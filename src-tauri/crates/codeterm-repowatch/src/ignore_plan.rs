use codeterm_util::watch_plan::DirectoryPlan;
use git2::Repository;
use std::path::{Path, PathBuf};

#[cfg(test)]
pub(crate) fn plan(root: &Path) -> Result<DirectoryPlan, String> {
    let inputs = Inputs::read(root)?;
    build(root, &inputs)
}

pub(crate) fn build(root: &Path, inputs: &Inputs) -> Result<DirectoryPlan, String> {
    #[cfg(test)]
    BUILDS.with(|count| count.set(count.get() + 1));
    codeterm_util::watch_plan::directory_plan(root, &inputs.tracked, &inputs.sources)
}

#[cfg(test)]
thread_local! { pub(crate) static BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[derive(PartialEq, Eq)]
pub(crate) struct Inputs {
    pub tracked: Vec<PathBuf>,
    pub sources: Vec<PathBuf>,
    pub index: PathBuf,
    pub config: PathBuf,
    pub gitdir: PathBuf,
    pub common: PathBuf,
}

impl Inputs {
    pub(crate) fn read(root: &Path) -> Result<Self, String> {
        let repo = Repository::open(root).map_err(|error| error.to_string())?;
        let mut tracked = crate::tracked::paths(&repo)?;
        tracked.sort();
        tracked.dedup();
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
        Ok(Self {
            tracked,
            sources,
            index: repo.path().join("index"),
            config: repo.commondir().join("config"),
            gitdir: repo.path().to_path_buf(),
            common: repo.commondir().to_path_buf(),
        })
    }
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
