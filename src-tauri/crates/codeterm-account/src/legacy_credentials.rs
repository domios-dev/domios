use crate::{Error, Result, Secret};

#[cfg(windows)]
mod platform {
    use super::*;
    use windows_sys::Win32::{
        Foundation::{GetLastError, ERROR_NOT_FOUND},
        Security::Credentials::*,
    };
    fn target(namespace: &str, name: &str) -> Vec<u16> {
        format!("Domios:{namespace}:{name}\0")
            .encode_utf16()
            .collect()
    }
    pub fn load(namespace: &str, name: &str) -> Result<Option<Secret>> {
        let target = target(namespace, name);
        let mut credential = std::ptr::null_mut();
        unsafe {
            if CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) == 0 {
                return if GetLastError() == ERROR_NOT_FOUND {
                    Ok(None)
                } else {
                    Err(Error::Credentials)
                };
            }
            let value = &*credential;
            let bytes = if value.CredentialBlobSize == 0 {
                &[]
            } else {
                std::slice::from_raw_parts(value.CredentialBlob, value.CredentialBlobSize as usize)
            };
            let token = std::str::from_utf8(bytes)
                .map(Secret::new)
                .map_err(|_| Error::Credentials);
            CredFree(credential.cast());
            token.map(Some)
        }
    }
    pub fn delete(namespace: &str, name: &str) -> Result<()> {
        if unsafe { CredDeleteW(target(namespace, name).as_ptr(), CRED_TYPE_GENERIC, 0) } != 0
            || unsafe { GetLastError() } == ERROR_NOT_FOUND
        {
            Ok(())
        } else {
            Err(Error::Credentials)
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use security_framework::item::{ItemClass, ItemSearchOptions, SearchResult};
    fn query(namespace: &str, name: &str) -> ItemSearchOptions {
        let mut query = ItemSearchOptions::new();
        query
            .class(ItemClass::generic_password())
            .service(&format!("Domios:{namespace}"))
            .account(name)
            .skip_authenticated_items(true);
        query
    }
    pub fn load(namespace: &str, name: &str) -> Result<Option<Secret>> {
        match query(namespace, name).load_data(true).search() {
            Ok(items) => Ok(items.into_iter().find_map(|item| match item {
                SearchResult::Data(bytes) => String::from_utf8(bytes).ok().map(Secret::new),
                _ => None,
            })),
            Err(error) if error.code() == -25300 => Ok(None),
            Err(_) => Err(Error::Credentials),
        }
    }
    pub fn delete(namespace: &str, name: &str) -> Result<()> {
        match query(namespace, name).delete() {
            Ok(()) => Ok(()),
            Err(error) if error.code() == -25300 => Ok(()),
            Err(_) => Err(Error::Credentials),
        }
    }
}

#[cfg(any(windows, target_os = "macos"))]
pub async fn load(namespace: String, name: String) -> Result<Option<Secret>> {
    tokio::task::spawn_blocking(move || platform::load(&namespace, &name))
        .await
        .map_err(|_| Error::Credentials)?
}
#[cfg(any(windows, target_os = "macos"))]
pub async fn delete(namespace: String, name: String) -> Result<()> {
    tokio::task::spawn_blocking(move || platform::delete(&namespace, &name))
        .await
        .map_err(|_| Error::Credentials)?
}

#[cfg(target_os = "linux")]
async fn command(action: &str, namespace: &str, name: &str) -> Result<std::process::Output> {
    let mut command = tokio::process::Command::new("secret-tool");
    command.arg(action);
    if action == "search" {
        command.arg("--all");
    }
    command
        .args(["service", "Domios", "instance", namespace, "account", name])
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    command.output().await.map_err(|_| Error::Credentials)
}
#[cfg(target_os = "linux")]
pub async fn load(namespace: String, name: String) -> Result<Option<Secret>> {
    // Search loads unlocked secrets without requesting the unlock prompt used by lookup.
    let output = command("search", &namespace, &name).await?;
    let body = String::from_utf8(output.stdout).map_err(|_| Error::Credentials)?;
    Ok(body
        .lines()
        .find_map(|line| line.strip_prefix("secret = ").map(Secret::new)))
}
#[cfg(target_os = "linux")]
pub async fn delete(namespace: String, name: String) -> Result<()> {
    let output = command("clear", &namespace, &name).await?;
    if output.status.success() || output.status.code() == Some(1) {
        Ok(())
    } else {
        Err(Error::Credentials)
    }
}
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub async fn load(_: String, _: String) -> Result<Option<Secret>> {
    Ok(None)
}
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub async fn delete(_: String, _: String) -> Result<()> {
    Ok(())
}
