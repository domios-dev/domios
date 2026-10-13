use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserFamily {
    Chrome,
    Chromium,
    Edge,
    Configured,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub family: BrowserFamily,
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocatedBrowser {
    pub family: BrowserFamily,
    pub binary: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocateError {
    ConfiguredMissing(PathBuf),
    NotFound { searched: usize },
}

impl std::fmt::Display for LocateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConfiguredMissing(path) => {
                write!(f, "configured browser binary {} does not exist", path.display())
            }
            Self::NotFound { searched } => write!(
                f,
                "no Chrome, Chromium or Edge binary found ({searched} locations searched); configure a browser path"
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostOs {
    Windows,
    MacOs,
    Linux,
}

impl HostOs {
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct EnvFacts {
    pub program_files: Option<PathBuf>,
    pub program_files_x86: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
    pub home: Option<PathBuf>,
}

impl EnvFacts {
    pub fn current() -> Self {
        Self {
            program_files: std::env::var_os("ProgramFiles").map(PathBuf::from),
            program_files_x86: std::env::var_os("ProgramFiles(x86)").map(PathBuf::from),
            local_app_data: std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
            home: dirs::home_dir(),
        }
    }
}

/// Every candidate binary for `os`, in preference order (Chrome, Chromium, Edge last).
pub fn candidates(os: HostOs, env: &EnvFacts) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut push = |family, path: PathBuf| {
        if !out.iter().any(|c: &Candidate| c.path == path) {
            out.push(Candidate { family, path });
        }
    };
    match os {
        HostOs::Windows => {
            let roots: Vec<&PathBuf> = [
                &env.program_files,
                &env.program_files_x86,
                &env.local_app_data,
            ]
            .into_iter()
            .flatten()
            .collect();
            for root in &roots {
                push(
                    BrowserFamily::Chrome,
                    root.join("Google/Chrome/Application/chrome.exe"),
                );
            }
            if let Some(local) = &env.local_app_data {
                push(
                    BrowserFamily::Chromium,
                    local.join("Chromium/Application/chrome.exe"),
                );
            }
            for root in &roots {
                push(
                    BrowserFamily::Edge,
                    root.join("Microsoft/Edge/Application/msedge.exe"),
                );
            }
        }
        HostOs::MacOs => {
            let mut app_dirs = vec![PathBuf::from("/Applications")];
            if let Some(home) = &env.home {
                app_dirs.push(home.join("Applications"));
            }
            for dir in &app_dirs {
                push(
                    BrowserFamily::Chrome,
                    dir.join("Google Chrome.app/Contents/MacOS/Google Chrome"),
                );
            }
            for dir in &app_dirs {
                push(
                    BrowserFamily::Chromium,
                    dir.join("Chromium.app/Contents/MacOS/Chromium"),
                );
            }
            for dir in &app_dirs {
                push(
                    BrowserFamily::Edge,
                    dir.join("Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
                );
            }
        }
        HostOs::Linux => {
            for path in [
                "/usr/bin/google-chrome",
                "/usr/bin/google-chrome-stable",
                "/opt/google/chrome/chrome",
            ] {
                push(BrowserFamily::Chrome, PathBuf::from(path));
            }
            for path in [
                "/usr/bin/chromium",
                "/usr/bin/chromium-browser",
                "/snap/bin/chromium",
            ] {
                push(BrowserFamily::Chromium, PathBuf::from(path));
            }
            for path in [
                "/usr/bin/microsoft-edge",
                "/usr/bin/microsoft-edge-stable",
                "/opt/microsoft/msedge/msedge",
            ] {
                push(BrowserFamily::Edge, PathBuf::from(path));
            }
        }
    }
    out
}

/// Pure decision: a configured path wins (and must exist); otherwise the first existing candidate.
pub fn choose(
    configured: Option<&Path>,
    candidates: &[Candidate],
    exists: impl Fn(&Path) -> bool,
) -> Result<LocatedBrowser, LocateError> {
    if let Some(path) = configured.filter(|path| !path.as_os_str().is_empty()) {
        return if exists(path) {
            Ok(LocatedBrowser {
                family: BrowserFamily::Configured,
                binary: path.to_path_buf(),
            })
        } else {
            Err(LocateError::ConfiguredMissing(path.to_path_buf()))
        };
    }
    candidates
        .iter()
        .find(|candidate| exists(&candidate.path))
        .map(|candidate| LocatedBrowser {
            family: candidate.family,
            binary: candidate.path.clone(),
        })
        .ok_or(LocateError::NotFound {
            searched: candidates.len(),
        })
}

pub fn locate(configured: Option<&Path>) -> Result<LocatedBrowser, LocateError> {
    let list = candidates(HostOs::current(), &EnvFacts::current());
    choose(configured, &list, |path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn windows_env() -> EnvFacts {
        EnvFacts {
            program_files: Some(PathBuf::from("C:/Program Files")),
            program_files_x86: Some(PathBuf::from("C:/Program Files (x86)")),
            local_app_data: Some(PathBuf::from("C:/Users/u/AppData/Local")),
            home: Some(PathBuf::from("C:/Users/u")),
        }
    }

    fn existing(paths: &[&str]) -> impl Fn(&Path) -> bool {
        let set: HashSet<PathBuf> = paths.iter().map(PathBuf::from).collect();
        move |path: &Path| set.contains(path)
    }

    #[test]
    fn browser_locator_prefers_chrome_over_edge_on_windows() {
        let list = candidates(HostOs::Windows, &windows_env());
        let found = choose(
            None,
            &list,
            existing(&[
                "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
                "C:/Program Files/Google/Chrome/Application/chrome.exe",
            ]),
        )
        .unwrap();
        assert_eq!(found.family, BrowserFamily::Chrome);
    }

    #[test]
    fn browser_locator_falls_back_to_edge_only_when_nothing_else_exists() {
        let list = candidates(HostOs::Windows, &windows_env());
        let found = choose(
            None,
            &list,
            existing(&["C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe"]),
        )
        .unwrap();
        assert_eq!(found.family, BrowserFamily::Edge);
    }

    #[test]
    fn browser_locator_orders_families_chrome_chromium_edge_on_every_os() {
        for os in [HostOs::Windows, HostOs::MacOs, HostOs::Linux] {
            let families: Vec<BrowserFamily> = candidates(os, &windows_env())
                .into_iter()
                .map(|candidate| candidate.family)
                .collect();
            let first_edge = families
                .iter()
                .position(|f| *f == BrowserFamily::Edge)
                .unwrap();
            let last_chrome = families
                .iter()
                .rposition(|f| *f == BrowserFamily::Chrome)
                .unwrap();
            let last_chromium = families
                .iter()
                .rposition(|f| *f == BrowserFamily::Chromium)
                .unwrap();
            assert!(last_chrome < first_edge, "{os:?}");
            assert!(last_chromium < first_edge, "{os:?}");
            assert!(
                last_chrome
                    < families
                        .iter()
                        .position(|f| *f == BrowserFamily::Chromium)
                        .unwrap()
            );
        }
    }

    #[test]
    fn browser_locator_configured_path_wins_over_candidates() {
        let list = candidates(HostOs::Linux, &EnvFacts::default());
        let found = choose(
            Some(Path::new("/opt/cft/chrome")),
            &list,
            existing(&["/opt/cft/chrome", "/usr/bin/google-chrome"]),
        )
        .unwrap();
        assert_eq!(found.family, BrowserFamily::Configured);
        assert_eq!(found.binary, PathBuf::from("/opt/cft/chrome"));
    }

    #[test]
    fn browser_locator_missing_configured_path_is_an_error_not_a_fallthrough() {
        let list = candidates(HostOs::Linux, &EnvFacts::default());
        let error = choose(
            Some(Path::new("/nope/chrome")),
            &list,
            existing(&["/usr/bin/google-chrome"]),
        )
        .unwrap_err();
        assert_eq!(
            error,
            LocateError::ConfiguredMissing(PathBuf::from("/nope/chrome"))
        );
    }

    #[test]
    fn browser_locator_empty_configured_path_means_unset() {
        let list = candidates(HostOs::Linux, &EnvFacts::default());
        let found = choose(Some(Path::new("")), &list, existing(&["/usr/bin/chromium"])).unwrap();
        assert_eq!(found.family, BrowserFamily::Chromium);
    }

    #[test]
    fn browser_locator_reports_not_found_with_search_count() {
        let list = candidates(HostOs::MacOs, &EnvFacts::default());
        let error = choose(None, &list, existing(&[])).unwrap_err();
        assert_eq!(
            error,
            LocateError::NotFound {
                searched: list.len()
            }
        );
    }

    #[test]
    fn browser_locator_windows_without_env_has_no_candidates() {
        assert!(candidates(HostOs::Windows, &EnvFacts::default()).is_empty());
    }
}
