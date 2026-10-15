#[derive(Debug, Default)]
pub struct ExecutionPolicyValues {
    pub policies: [Option<String>; 4],
    pub enable_scripts: [Option<u32>; 2],
}

#[cfg(windows)]
pub fn read_execution_policy_values() -> &'static ExecutionPolicyValues {
    static VALUES: std::sync::OnceLock<ExecutionPolicyValues> = std::sync::OnceLock::new();
    VALUES.get_or_init(load_execution_policy_values)
}

#[cfg(windows)]
fn load_execution_policy_values() -> ExecutionPolicyValues {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let group_path = r"Software\Policies\Microsoft\Windows\PowerShell";
    let preference_path = r"Software\Microsoft\PowerShell\1\ShellIds\Microsoft.PowerShell";
    let locations = [
        (HKEY_LOCAL_MACHINE, group_path),
        (HKEY_CURRENT_USER, group_path),
        (HKEY_CURRENT_USER, preference_path),
        (HKEY_LOCAL_MACHINE, preference_path),
    ];
    let mut values = ExecutionPolicyValues::default();
    for (index, (hive, path)) in locations.into_iter().enumerate() {
        match RegKey::predef(hive).open_subkey(path) {
            Ok(key) => {
                values.policies[index] = key.get_value("ExecutionPolicy").ok();
                if index < 2 {
                    values.enable_scripts[index] = key.get_value("EnableScripts").ok();
                }
            }
            Err(error) => log::trace!("PowerShell policy registry read {path}: {error}"),
        }
    }
    values
}
