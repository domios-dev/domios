use crate::service::ServiceContext;
use codeterm_types::plan::{ExecutionIntent, ExecutionSandboxPolicy};
use std::{
    collections::HashMap,
    env,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

pub(super) fn configure(
    svc: &ServiceContext,
    tab_id: &str,
    environment: &mut HashMap<String, String>,
) -> Result<(), String> {
    let selected = svc.mux.read().get_tab(tab_id).is_some_and(|tab| {
        selects_native_shell(
            tab.neighbor_authority.is_some(),
            tab.restorable_launch.execution_sandbox.as_ref(),
            tab.restorable_launch.permission_bypass_requested,
        )
    });
    if !selected {
        return Ok(());
    }

    let path = environment
        .get("PATH")
        .or_else(|| {
            environment
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
                .map(|(_, path)| path)
        })
        .map(OsString::from)
        .or_else(|| env::var_os("PATH"))
        .unwrap_or_default();
    let core_dirs = ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"]
        .into_iter()
        .filter_map(env::var_os)
        .map(|root| PathBuf::from(root).join("PowerShell/7"));
    let windows_dir = env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join("System32/WindowsPowerShell/v1.0"));
    let Some(filtered) = resolve_path(&path, core_dirs, windows_dir, |path| {
        let target = path.canonicalize().ok()?;
        target.is_file().then_some(target)
    })?
    else {
        return Ok(());
    };
    environment.retain(|key, _| !key.eq_ignore_ascii_case("PATH"));
    environment.insert("PATH".into(), filtered);
    Ok(())
}

fn selects_native_shell(
    neighbor: bool,
    sandbox: Option<&ExecutionSandboxPolicy>,
    full: bool,
) -> bool {
    neighbor
        && match sandbox {
            Some(policy) => {
                policy.intent == ExecutionIntent::Mutating
                    && policy.runtime_mode.as_deref() == Some("workspace-write")
            }
            None => full,
        }
}

fn resolve_path(
    path: &OsStr,
    core_dirs: impl IntoIterator<Item = PathBuf>,
    windows_dir: Option<PathBuf>,
    canonicalize: impl Fn(&Path) -> Option<PathBuf>,
) -> Result<Option<String>, String> {
    let mut alias_found = false;
    let mut dirs: Vec<_> = env::split_paths(path)
        .filter(|dir| {
            let alias = ["pwsh.exe", "powershell.exe"].into_iter().any(|name| {
                let shell = dir.join(name);
                is_store_shell(&shell)
                    || canonicalize(&shell).is_some_and(|target| is_store_shell(&target))
            });
            alias_found |= alias;
            !alias
        })
        .collect();
    if !alias_found {
        return Ok(None);
    }

    let native = |dir: &Path, name: &str| {
        let shell = dir.join(name);
        dir.is_absolute()
            && !is_store_shell(&shell)
            && canonicalize(&shell).is_some_and(|target| !is_store_shell(&target))
    };
    if !dirs.iter().any(|dir| native(dir, "pwsh.exe")) {
        if let Some(dir) = core_dirs.into_iter().find(|dir| native(dir, "pwsh.exe")) {
            dirs.insert(0, dir);
        } else {
            let dir = windows_dir
                .filter(|dir| native(dir, "powershell.exe"))
                .ok_or("Native Windows PowerShell unavailable for concierge runtime")?;
            dirs.retain(|entry| entry != &dir);
            dirs.insert(0, dir);
        }
    }
    env::join_paths(dirs)
        .ok()
        .and_then(|path| path.into_string().ok())
        .map(Some)
        .ok_or_else(|| "Concierge runtime PATH could not be represented".into())
}

fn is_store_shell(path: &Path) -> bool {
    path.as_os_str()
        .to_string_lossy()
        .split(['/', '\\'])
        .skip_while(|component| !component.eq_ignore_ascii_case("WindowsApps"))
        .nth(1)
        .is_some_and(|component| {
            component.eq_ignore_ascii_case("pwsh.exe")
                || component.eq_ignore_ascii_case("powershell.exe")
                || component
                    .to_ascii_lowercase()
                    .starts_with("microsoft.powershell")
        })
}

#[cfg(test)]
mod tests;
