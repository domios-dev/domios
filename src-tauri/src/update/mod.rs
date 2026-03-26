pub mod checker;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum InstallMethod {
    Brew,
    Scoop,
    Dmg,
    Nsis,
    Msi,
    Deb,
    Rpm,
    AppImage,
    Unknown,
}

impl std::fmt::Display for InstallMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Brew => write!(f, "brew"),
            Self::Scoop => write!(f, "scoop"),
            Self::Dmg => write!(f, "dmg"),
            Self::Nsis => write!(f, "nsis"),
            Self::Msi => write!(f, "msi"),
            Self::Deb => write!(f, "deb"),
            Self::Rpm => write!(f, "rpm"),
            Self::AppImage => write!(f, "appimage"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

pub struct UpdateState {
    pub install_method: InstallMethod,
    pub last_check: Option<checker::UpdateInfo>,
    pub last_check_time: Option<std::time::Instant>,
}

/// Detect how CodeTerm was installed based on executable path.
pub fn detect_install_method() -> InstallMethod {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return InstallMethod::Unknown,
    };
    let path = exe.to_string_lossy();

    #[cfg(target_os = "macos")]
    {
        if path.contains("/opt/homebrew/") || path.contains("/usr/local/Caskroom/") {
            return InstallMethod::Brew;
        }
        if path.contains("/Applications/") {
            return InstallMethod::Dmg;
        }
    }

    #[cfg(target_os = "windows")]
    {
        let path_lower = path.to_lowercase();
        if path_lower.contains(r"\scoop\apps\") {
            return InstallMethod::Scoop;
        }
        if path_lower.contains(r"\program files\") || path_lower.contains(r"\program files (x86)\") {
            return InstallMethod::Msi;
        }
        return InstallMethod::Nsis;
    }

    #[cfg(target_os = "linux")]
    {
        if std::env::var("APPIMAGE").is_ok() {
            return InstallMethod::AppImage;
        }
        if path.starts_with("/usr/") {
            if std::path::Path::new("/usr/bin/dpkg").exists() {
                return InstallMethod::Deb;
            }
            if std::path::Path::new("/usr/bin/rpm").exists() {
                return InstallMethod::Rpm;
            }
        }
    }

    InstallMethod::Unknown
}
