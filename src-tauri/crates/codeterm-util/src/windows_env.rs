use std::ffi::OsString;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvironmentValue {
    Missing,
    Value(OsString),
    Unreadable(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistedPath {
    pub machine: EnvironmentValue,
    pub user: EnvironmentValue,
}

#[cfg(windows)]
pub fn read_persisted_path() -> PersistedPath {
    PersistedPath {
        machine: read_path_value(
            winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE),
            "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment",
        ),
        user: read_path_value(
            winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER),
            "Environment",
        ),
    }
}

#[cfg(not(windows))]
pub fn read_persisted_path() -> PersistedPath {
    PersistedPath {
        machine: EnvironmentValue::Missing,
        user: EnvironmentValue::Missing,
    }
}

#[cfg(windows)]
fn read_path_value(root: winreg::RegKey, subkey: &str) -> EnvironmentValue {
    use std::io::ErrorKind;

    let key = match root.open_subkey(subkey) {
        Ok(key) => key,
        Err(error) if error.kind() == ErrorKind::NotFound => return EnvironmentValue::Missing,
        Err(error) => return EnvironmentValue::Unreadable(error.to_string()),
    };

    let raw = match key
        .enum_values()
        .find_map(|entry| match entry {
            Ok((name, value)) if name.eq_ignore_ascii_case("Path") => Some(Ok(value)),
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .transpose()
    {
        Ok(Some(value)) => value,
        Ok(None) => return EnvironmentValue::Missing,
        Err(error) => return EnvironmentValue::Unreadable(error.to_string()),
    };

    match registry_value_to_string(&raw) {
        Ok(value) => EnvironmentValue::Value(value),
        Err(error) => EnvironmentValue::Unreadable(error.to_string()),
    }
}

#[cfg(windows)]
fn registry_value_to_string(value: &winreg::RegValue) -> std::io::Result<OsString> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use winreg::enums::RegType;
    use winreg::types::FromRegValue;
    use winapi::um::processenv::ExpandEnvironmentStringsW;

    if !matches!(value.vtype, RegType::REG_SZ | RegType::REG_EXPAND_SZ) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "PATH registry value is not a string",
        ));
    }
    let source = OsString::from_reg_value(value)?;
    if value.vtype != RegType::REG_EXPAND_SZ {
        return Ok(source);
    }

    let input: Vec<u16> = source
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let required = unsafe { ExpandEnvironmentStringsW(input.as_ptr(), std::ptr::null_mut(), 0) };
    if required == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut output = vec![0u16; required as usize];
    let written = unsafe {
        ExpandEnvironmentStringsW(input.as_ptr(), output.as_mut_ptr(), output.len() as u32)
    };
    if written == 0 {
        return Err(std::io::Error::last_os_error());
    }
    output.truncate(written.saturating_sub(1) as usize);
    Ok(OsString::from_wide(&output))
}

#[cfg(all(test, windows))]
mod tests {
    use super::registry_value_to_string;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStrExt;

    #[test]
    fn expands_expandable_path_values_without_touching_the_registry() {
        let root = std::env::var_os("SystemRoot").expect("Windows supplies SystemRoot");
        let raw = OsString::from(r"%SystemRoot%\codeterm-resolver-fixture");
        let bytes = raw
            .encode_wide()
            .chain(std::iter::once(0))
            .flat_map(u16::to_ne_bytes)
            .collect();
        let value = winreg::RegValue {
            bytes,
            vtype: winreg::enums::RegType::REG_EXPAND_SZ,
        };

        let expanded = registry_value_to_string(&value).unwrap();
        let expected = std::path::PathBuf::from(root).join("codeterm-resolver-fixture");
        assert_eq!(expanded, expected.into_os_string());
    }
}
