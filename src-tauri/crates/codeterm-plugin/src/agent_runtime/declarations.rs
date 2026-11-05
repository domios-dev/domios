use super::*;
use std::collections::HashSet;
fn invalid() -> RuntimeFailure {
    RuntimeFailure::new(
        RuntimeFailureCode::InvalidSchema,
        "invalid runtime declaration",
    )
}
fn text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
fn unique<'a>(values: impl Iterator<Item = &'a str>) -> bool {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .all(|value| text(value) && seen.insert(value))
}
pub(super) fn validate_settings(settings: &RuntimeSettings) -> Result<(), RuntimeFailure> {
    if let Some(commands) = &settings.commands {
        if commands.len() > 128
            || !unique(commands.iter().map(|v| v.name.as_str()))
            || commands.iter().any(|v| {
                v.description.len() > 4096
                    || v.argument_hint.as_ref().is_some_and(|s| s.len() > 4096)
            })
        {
            return Err(invalid());
        }
    }
    if let Some(options) = &settings.config_options {
        if options.len() > 64
            || !unique(options.iter().map(|v| v.id.as_str()))
            || options.iter().any(|v| {
                !text(&v.name)
                    || v.category.as_ref().is_some_and(|value| !text(value))
                    || !text(&v.current_value)
                    || v.options.len() > 128
                    || !unique(v.options.iter().map(|v| v.value.as_str()))
                    || v.options.iter().any(|v| !text(&v.name))
            })
        {
            return Err(invalid());
        }
    }
    if let Some(modes) = &settings.modes {
        if modes.len() > 64
            || !unique(modes.iter().map(|v| v.id.as_str()))
            || modes.iter().any(|v| !text(&v.name))
        {
            return Err(invalid());
        }
    }
    Ok(())
}
pub(super) fn validate_authentication(auth: &RuntimeAuthentication) -> Result<(), RuntimeFailure> {
    if auth.methods.len() > 32
        || !unique(auth.methods.iter().map(|v| v.id.as_str()))
        || auth.methods.iter().any(|v| !text(&v.name))
    {
        return Err(invalid());
    }
    Ok(())
}
