use super::*;

pub(super) fn validate(ctx: &RuntimeContext) -> Result<(), RuntimeFailure> {
    if let Some(policy) = &ctx.restricted_cli {
        let hash = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        };
        if policy.tab_id != ctx.tab_id
            || policy.binding_nonce != ctx.binding_nonce
            || policy.epoch != ctx.epoch
            || policy.cwd != ctx.cwd
            || !RestrictedCliCapability::valid_metadata_directory(&policy.metadata_directory)
            || !matches!(
                ctx.settings.mode.as_deref(),
                Some("workspace-write" | "read-only-with-writable-roots")
            )
            || ctx.settings.network_access != Some(false)
            || !identifier(&policy.executable)
            || !policy
                .executable
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"/\\:._-~".contains(&byte))
            || !(policy.executable.starts_with('/')
                || std::path::Path::new(&policy.executable).is_absolute())
            || !hash(&policy.executable_sha256)
            || !hash(&policy.config_sha256)
            || !hash(&policy.rule_sha256)
        {
            return Err(mismatch(
                "restricted CLI exception is outside runtime authority",
            ));
        }
    }
    Ok(())
}
