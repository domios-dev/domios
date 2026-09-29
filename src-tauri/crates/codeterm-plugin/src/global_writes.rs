pub fn machine_global_write_allowed() -> bool {
    let configured = std::env::var("CODETERM_MACHINE_OWNER").ok();
    let instance = std::env::var("CODETERM_INSTANCE").ok();
    machine_global_write_allowed_for(cfg!(debug_assertions), instance.as_deref(), configured.as_deref())
}

fn machine_global_write_allowed_for(
    debug_build: bool,
    instance: Option<&str>,
    configured_owner: Option<&str>,
) -> bool {
    match configured_owner {
        Some("true") => true,
        Some("false") => false,
        _ => !debug_build && instance.is_none_or(|name| name.trim().is_empty()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_unnamed_production_instance_owns_machine_global_files_by_default() {
        assert!(!machine_global_write_allowed_for(true, None, None));
        assert!(!machine_global_write_allowed_for(false, Some("worktree"), None));
        assert!(!machine_global_write_allowed_for(false, None, Some("false")));
        assert!(machine_global_write_allowed_for(true, Some("dev"), Some("true")));
        assert!(machine_global_write_allowed_for(false, None, None));
    }
}
