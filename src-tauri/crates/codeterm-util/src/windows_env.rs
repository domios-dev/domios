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

    let raw = match key.get_raw_value("Path") {
        Ok(value) => value,
        Err(error) if error.kind() == ErrorKind::NotFound => return EnvironmentValue::Missing,
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

    expand_environment_strings_with(&source, |input, output| {
        let (output, capacity) = match output {
            Some(buffer) => (buffer.as_mut_ptr(), buffer.len() as u32),
            None => (std::ptr::null_mut(), 0),
        };
        let written = unsafe {
            ExpandEnvironmentStringsW(input.as_ptr(), output, capacity)
        };
        if written == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(written as usize)
        }
    })
}

#[cfg(windows)]
fn expand_environment_strings_with<F>(source: &std::ffi::OsStr, mut expand: F) -> std::io::Result<OsString>
where
    F: FnMut(&[u16], Option<&mut [u16]>) -> std::io::Result<usize>,
{
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    let input: Vec<u16> = source
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let required = expand(&input, None)?;
    if required == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "expanded PATH size query returned zero",
        ));
    }
    let mut output = vec![0u16; required];
    let written = expand(&input, Some(&mut output))?;
    if written > output.len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "expanded PATH did not fit the queried buffer",
        ));
    }
    if written == 0 || output.get(written - 1) != Some(&0) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "expanded PATH was not terminated",
        ));
    }
    output.truncate(written - 1);
    Ok(OsString::from_wide(&output))
}

#[cfg(all(test, windows))]
mod tests {
    use super::{expand_environment_strings_with, registry_value_to_string};
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

    #[test]
    fn converts_expansion_with_an_injected_size_and_write_fixture() {
        let source = OsString::from("%fixture%");
        let expected: Vec<u16> = OsString::from("C:\\fixture\\bin")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut calls = 0;
        let expanded = expand_environment_strings_with(&source, |_, output| {
            calls += 1;
            match output {
                None => Ok(expected.len()),
                Some(buffer) => {
                    buffer.copy_from_slice(&expected);
                    Ok(expected.len())
                }
            }
        })
        .unwrap();

        assert_eq!(calls, 2);
        assert_eq!(expanded, OsString::from("C:\\fixture\\bin"));
    }

    #[test]
    fn rejects_an_expansion_that_grows_after_the_size_query() {
        let source = OsString::from("%fixture%");
        let error = expand_environment_strings_with(&source, |_, output| match output {
            None => Ok(2),
            Some(buffer) => Ok(buffer.len() + 1),
        })
        .expect_err("a second-call size increase must not be accepted as a truncated PATH");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "expanded PATH did not fit the queried buffer");
    }
}
