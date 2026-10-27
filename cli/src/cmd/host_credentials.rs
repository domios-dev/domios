use std::path::Path;

pub(super) fn read(data_root: &Path, remote: bool, caller_tab: Option<&str>) -> Result<Option<String>, String> {
    admit(remote, caller_tab)?;
    codeterm_util::instance_token::read_at(data_root)
}

fn admit(remote: bool, caller_tab: Option<&str>) -> Result<(), String> {
    if remote {
        return Err("Instance API credentials are local-only; run on the owning machine".into());
    }
    if caller_tab.is_some() {
        return Err("Instance API credentials and token-bearing web links are host-only; tabs use their launch credential".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
