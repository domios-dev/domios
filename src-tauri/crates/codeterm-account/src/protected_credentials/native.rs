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
    pub fn save(namespace: &str, name: &str, token: &Secret) -> Result<()> {
        let mut target = target(namespace, name);
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            CredentialBlobSize: token
                .expose()
                .len()
                .try_into()
                .map_err(|_| Error::Credentials)?,
            CredentialBlob: token.expose().as_ptr().cast_mut(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            ..Default::default()
        };
        if unsafe { CredWriteW(&credential, 0) } != 0 {
            Ok(())
        } else {
            Err(Error::Credentials)
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
    use security_framework::passwords;
    fn service(namespace: &str) -> String {
        format!("Domios:{namespace}")
    }
    pub fn load(namespace: &str, name: &str) -> Result<Option<Secret>> {
        match passwords::get_generic_password(&service(namespace), name) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Secret::new)
                .map(Some)
                .map_err(|_| Error::Credentials),
            Err(error) if error.code() == -25300 => Ok(None),
            Err(_) => Err(Error::Credentials),
        }
    }
    pub fn save(namespace: &str, name: &str, token: &Secret) -> Result<()> {
        passwords::set_generic_password(&service(namespace), name, token.expose().as_bytes())
            .map_err(|_| Error::Credentials)
    }
    pub fn delete(namespace: &str, name: &str) -> Result<()> {
        match passwords::delete_generic_password(&service(namespace), name) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == -25300 => Ok(()),
            Err(_) => Err(Error::Credentials),
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    fn command(action: &str, namespace: &str, name: &str) -> Command {
        let mut command = Command::new("secret-tool");
        command.arg(action);
        if action == "store" {
            command.args(["--label", "Domios sign-in"]);
        }
        command.args(["service", "Domios", "instance", namespace, "account", name]);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        command
    }
    pub fn load(namespace: &str, name: &str) -> Result<Option<Secret>> {
        let output = command("lookup", namespace, name)
            .output()
            .map_err(|_| Error::Credentials)?;
        if !output.status.success() {
            return Err(Error::Credentials);
        }
        String::from_utf8(output.stdout)
            .map(|value| Some(Secret::new(value.trim_end_matches('\n'))))
            .map_err(|_| Error::Credentials)
    }
    pub fn save(namespace: &str, name: &str, token: &Secret) -> Result<()> {
        let mut child = command("store", namespace, name)
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|_| Error::Credentials)?;
        let written = child
            .stdin
            .take()
            .ok_or(Error::Credentials)?
            .write_all(token.expose().as_bytes());
        let status = child.wait().map_err(|_| Error::Credentials)?;
        if written.is_ok() && status.success() {
            Ok(())
        } else {
            Err(Error::Credentials)
        }
    }
    pub fn delete(namespace: &str, name: &str) -> Result<()> {
        // secret-tool returns 1 when no matching item exists.
        let status = command("clear", namespace, name)
            .status()
            .map_err(|_| Error::Credentials)?;
        if status.success() || status.code() == Some(1) {
            Ok(())
        } else {
            Err(Error::Credentials)
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod platform {
    use super::*;
    pub fn load(_: &str, _: &str) -> Result<Option<Secret>> {
        Err(Error::Credentials)
    }
    pub fn save(_: &str, _: &str, _: &Secret) -> Result<()> {
        Err(Error::Credentials)
    }
    pub fn delete(_: &str, _: &str) -> Result<()> {
        Ok(())
    }
}
pub use platform::{delete, load, save};
