/// Which PowerShell edition runs a new tab; each keeps its own execution policy.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PowerShellHost {
    #[default]
    Windows,
    Core,
}

#[derive(Debug, Default)]
pub struct ExecutionPolicyValues {
    pub host: PowerShellHost,
    pub policies: [Option<String>; 4],
    pub enable_scripts: [Option<u32>; 2],
}

pub fn host_of(shell: &str) -> PowerShellHost {
    // Split on both separators: a Windows shell path must classify the same on every host.
    let file = shell.rsplit(['\\', '/']).next().unwrap_or_default();
    let name = file
        .len()
        .checked_sub(4)
        .filter(|&stem| file.is_char_boundary(stem) && file[stem..].eq_ignore_ascii_case(".exe"))
        .map_or(file, |stem| &file[..stem]);
    if name.eq_ignore_ascii_case("pwsh") {
        PowerShellHost::Core
    } else {
        PowerShellHost::Windows
    }
}

/// The `Microsoft.PowerShell:ExecutionPolicy` value of a `powershell.config.json`.
pub fn config_execution_policy(text: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}'))
        .ok()?
        .get("Microsoft.PowerShell:ExecutionPolicy")?
        .as_str()
        .map(str::to_owned)
}

/// Preferred PowerShell for a new tab: a real PowerShell 7 install, then PATH.
#[cfg(windows)]
pub fn preferred_shell() -> Option<String> {
    // Avoid WindowsApps pwsh.exe — Store-packaged PowerShell can answer terminal
    // queries while leaking prompt output outside ConPTY. Prefer real installs.
    let candidates = [
        r"C:\Program Files\PowerShell\7\pwsh.exe",
        r"C:\Program Files (x86)\PowerShell\7\pwsh.exe",
    ];
    candidates
        .iter()
        .find(|candidate| std::path::Path::new(candidate).is_file())
        .map(|candidate| (*candidate).to_string())
        .or_else(|| which_exe("pwsh.exe"))
        .or_else(|| which_exe("powershell.exe"))
}

#[cfg(windows)]
pub fn which_exe(name: &str) -> Option<String> {
    let path = std::env::var("PATH").ok()?;
    path.split(';')
        .filter(|dir| !dir.is_empty() && !is_windowsapps_path(dir))
        .map(|dir| std::path::Path::new(dir).join(name))
        .find(|full| full.is_file())
        .map(|full| full.to_string_lossy().into_owned())
}

#[cfg(any(windows, test))]
fn is_windowsapps_path(path: &str) -> bool {
    path.to_ascii_lowercase().contains(r"\windowsapps")
}

/// Policy inputs for the PowerShell a new tab would run, read fresh so a
/// changed policy is seen on the next check.
#[cfg(windows)]
pub fn read_execution_policy_values() -> ExecutionPolicyValues {
    let shell = preferred_shell();
    match shell.as_deref().map(host_of) {
        Some(PowerShellHost::Core) => {
            load_core_policy_values(shell.as_deref().map(std::path::Path::new))
        }
        _ => load_windows_policy_values(),
    }
}

#[cfg(windows)]
const WINDOWS_GROUP_PATH: &str = r"Software\Policies\Microsoft\Windows\PowerShell";

#[cfg(windows)]
fn read_group_policy(values: &mut ExecutionPolicyValues, path: &str) {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    for (index, hive) in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER]
        .into_iter()
        .enumerate()
    {
        match RegKey::predef(hive).open_subkey(path) {
            Ok(key) => {
                values.policies[index] = key.get_value("ExecutionPolicy").ok();
                values.enable_scripts[index] = key.get_value("EnableScripts").ok();
            }
            Err(error) => log::trace!("PowerShell policy registry read {path}: {error}"),
        }
    }
}

#[cfg(windows)]
fn load_windows_policy_values() -> ExecutionPolicyValues {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let preference_path = r"Software\Microsoft\PowerShell\1\ShellIds\Microsoft.PowerShell";
    let mut values = ExecutionPolicyValues::default();
    read_group_policy(&mut values, WINDOWS_GROUP_PATH);
    for (index, hive) in [(2, HKEY_CURRENT_USER), (3, HKEY_LOCAL_MACHINE)] {
        match RegKey::predef(hive).open_subkey(preference_path) {
            Ok(key) => values.policies[index] = key.get_value("ExecutionPolicy").ok(),
            Err(error) => log::trace!("PowerShell policy registry read {preference_path}: {error}"),
        }
    }
    values
}

#[cfg(windows)]
fn load_core_policy_values(pwsh: Option<&std::path::Path>) -> ExecutionPolicyValues {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let core_group_path = r"Software\Policies\Microsoft\PowerShellCore";
    let defers_to_windows = [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER]
        .into_iter()
        .any(|hive| {
            RegKey::predef(hive)
                .open_subkey(core_group_path)
                .and_then(|key| key.get_value::<u32, _>("UseWindowsPowerShellPolicySetting"))
                .is_ok_and(|value| value == 1)
        });
    let mut values = ExecutionPolicyValues {
        host: PowerShellHost::Core,
        ..Default::default()
    };
    read_group_policy(
        &mut values,
        if defers_to_windows {
            WINDOWS_GROUP_PATH
        } else {
            core_group_path
        },
    );
    let read_config = |path: std::path::PathBuf| {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| config_execution_policy(&text))
    };
    values.policies[2] = documents_dir()
        .map(|documents| documents.join("PowerShell").join("powershell.config.json"))
        .and_then(read_config);
    values.policies[3] = pwsh
        .and_then(std::path::Path::parent)
        .map(|home| home.join("powershell.config.json"))
        .and_then(read_config);
    values
}

#[cfg(windows)]
fn documents_dir() -> Option<std::path::PathBuf> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Shell Folders")
        .and_then(|key| key.get_value::<String, _>("Personal"))
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .map(|profile| std::path::PathBuf::from(profile).join("Documents"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_follows_the_shell_executable_name() {
        assert_eq!(
            host_of(r"C:\Program Files\PowerShell\7\pwsh.exe"),
            PowerShellHost::Core
        );
        assert_eq!(host_of("PWSH"), PowerShellHost::Core);
        assert_eq!(
            host_of(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"),
            PowerShellHost::Windows
        );
    }

    #[test]
    fn config_policy_reads_only_the_execution_policy_key() {
        assert_eq!(
            config_execution_policy(
                "\u{feff}{\"Microsoft.PowerShell:ExecutionPolicy\":\"RemoteSigned\"}"
            )
            .as_deref(),
            Some("RemoteSigned")
        );
        assert_eq!(
            config_execution_policy("{\"ExecutionPolicy\":\"Bypass\"}"),
            None
        );
        assert_eq!(config_execution_policy("not json"), None);
    }

    #[test]
    fn windowsapps_paths_are_rejected_for_default_shell_lookup() {
        assert!(is_windowsapps_path(
            r"C:\Program Files\WindowsApps\Microsoft.PowerShell_7.6.3.0_x64__8wekyb3d8bbwe"
        ));
        assert!(is_windowsapps_path(
            r"C:\Users\dev\AppData\Local\Microsoft\WindowsApps"
        ));
        assert!(!is_windowsapps_path(r"C:\Program Files\PowerShell\7"));
    }
}
