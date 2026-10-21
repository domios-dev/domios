pub fn machine_global_write_allowed() -> bool {
    let configured = std::env::var("DOMIOS_MACHINE_OWNER")
        .ok()
        .or_else(|| std::env::var("CODETERM_MACHINE_OWNER").ok()); // rename-bridge
    let instance = std::env::var("DOMIOS_INSTANCE").ok();
    let previous_hook_generation = cfg!(debug_assertions)
        && crate::agent_claude::provider::integration::installed_1_14_9_hook_generation();
    machine_global_write_allowed_for(
        cfg!(debug_assertions),
        instance.as_deref(),
        configured.as_deref(),
        previous_hook_generation,
    )
}

fn machine_global_write_allowed_for(
    debug_build: bool,
    instance: Option<&str>,
    configured_owner: Option<&str>,
    previous_hook_generation: bool,
) -> bool {
    if debug_build && previous_hook_generation {
        return false;
    }
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
        assert!(!machine_global_write_allowed_for(true, None, None, false));
        assert!(!machine_global_write_allowed_for(
            false,
            Some("worktree"),
            None,
            false,
        ));
        assert!(!machine_global_write_allowed_for(
            false,
            None,
            Some("false"),
            false,
        ));
        assert!(machine_global_write_allowed_for(
            true,
            Some("dev"),
            Some("true"),
            false,
        ));
        assert!(machine_global_write_allowed_for(false, None, None, false));
        assert!(!machine_global_write_allowed_for(
            true,
            None,
            Some("true"),
            true
        ));
    }
}
