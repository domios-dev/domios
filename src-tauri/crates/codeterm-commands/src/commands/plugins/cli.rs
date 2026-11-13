use serde_json::Value;

use codeterm_plugins::{PluginAction, PluginChannelAction, PluginCtx};

/// Run a plugin-management operation in-process via the shared
/// `codeterm-plugins` crate. The daemon used to fork its own `codeterm` CLI
/// here (process spawn + the CLI re-deriving its data dir); the logic now lives
/// in a shared crate both call directly (AGENTS.md: the daemon never shells out
/// to its own CLI). Callers pass the same `["plugin", …, "--json"]` argument
/// vectors the subprocess took, so behaviour is identical.
///
/// Still synchronous and blocking (filesystem + `git` for channel clones) — every
/// caller invokes it from inside `spawn_blocking`, so it stays off the async
/// runtime exactly as the subprocess did.
pub(super) fn run_plugin_cli(args: &[impl AsRef<str>]) -> Result<Value, String> {
    let ctx = PluginCtx {
        data_root: codeterm_service_core::paths::data_dir(),
        version: codeterm_build_info::installed_version().map(str::to_string),
    };
    run_with_lifecycle(args, ctx, std::sync::Arc::new(DaemonInstallLifecycle))
}

pub(super) fn run_with_lifecycle(
    args: &[impl AsRef<str>],
    ctx: PluginCtx,
    lifecycle: std::sync::Arc<dyn codeterm_plugins::InstallLifecycle>,
) -> Result<Value, String> {
    let parts: Vec<&str> = args.iter().map(|a| a.as_ref()).collect();
    let action = parse_plugin_args(&parts)?;
    codeterm_plugins::with_install_lifecycle(lifecycle, || codeterm_plugins::run(ctx, action))
}

struct DaemonInstallLifecycle;

impl codeterm_plugins::InstallLifecycle for DaemonInstallLifecycle {
    fn install(
        &self,
        id: &str,
        destination: &std::path::Path,
        operation: &mut codeterm_plugins::InstallOperation<'_>,
    ) -> Result<(), String> {
        codeterm_plugin::js_runtime::hot_reload::with_quiesced_install(id, destination, |reload| {
            operation(reload)
        })
    }
}

pub(super) fn with_install_lifecycle<T>(operation: impl FnOnce() -> T) -> T {
    codeterm_plugins::with_install_lifecycle(std::sync::Arc::new(DaemonInstallLifecycle), operation)
}

/// Map the daemon's `["plugin", <verb>, …, "--json"]` argument vector to a typed
/// [`PluginAction`]. The daemon emits a closed vocabulary (list / install /
/// update / update-system / remove / channel …), built by
/// `action::plugin_action_args` and the snapshot rebuild — anything else is a
/// caller bug, not user input, so an unknown verb is an error.
fn parse_plugin_args(parts: &[&str]) -> Result<PluginAction, String> {
    let mut tokens: Vec<&str> = parts.iter().copied().filter(|p| *p != "--json").collect();
    if tokens.first() == Some(&"plugin") {
        tokens.remove(0);
    }
    let cmd = *tokens.first().ok_or("empty plugin command")?;
    let rest = &tokens[1..];
    match cmd {
        "list" => Ok(PluginAction::List),
        "install" => Ok(PluginAction::Install {
            id: first_positional(rest),
            from_dir: flag_value(rest, "--from-dir"),
            id_override: flag_value(rest, "--id-override"),
            channel: flag_value(rest, "--channel"),
        }),
        "update" => Ok(PluginAction::Update {
            id: first_positional(rest),
        }),
        "update-system" => Ok(PluginAction::UpdateSystem {
            id: first_positional(rest),
        }),
        "remove" => Ok(PluginAction::Remove {
            id: first_positional(rest).ok_or("plugin remove requires an id")?,
            force: has_flag(rest, "--force"),
            remove_data: has_flag(rest, "--remove-data"),
        }),
        "settings" => Ok(PluginAction::Settings {
            id: first_positional(rest).ok_or("plugin settings requires an id")?,
            allow_host: flag_values(rest, "--allow-host"),
            disallow_host: flag_values(rest, "--disallow-host"),
        }),
        "channel" => parse_channel_args(rest),
        other => Err(format!("unsupported in-process plugin command: {other}")),
    }
}

fn parse_channel_args(rest: &[&str]) -> Result<PluginAction, String> {
    let sub = *rest.first().ok_or("empty plugin channel command")?;
    let rest = &rest[1..];
    let action = match sub {
        "add" => PluginChannelAction::Add {
            url: first_positional(rest).ok_or("channel add requires a url")?,
            name: flag_value(rest, "--name"),
        },
        "ensure-default" => PluginChannelAction::EnsureDefault,
        "list" => PluginChannelAction::List,
        "refresh" => PluginChannelAction::Refresh {
            name: first_positional(rest),
        },
        "diff" => PluginChannelAction::Diff {
            name: first_positional(rest),
        },
        "remove" => PluginChannelAction::Remove {
            name: first_positional(rest).ok_or("channel remove requires a name")?,
            force: has_flag(rest, "--force"),
        },
        other => return Err(format!("unsupported in-process channel command: {other}")),
    };
    Ok(PluginAction::Channel(action))
}

fn first_positional(rest: &[&str]) -> Option<String> {
    let mut tokens = rest.iter();
    while let Some(token) = tokens.next() {
        if matches!(
            *token,
            "--from-dir"
                | "--id-override"
                | "--channel"
                | "--name"
                | "--allow-host"
                | "--disallow-host"
        ) {
            tokens.next();
        } else if !token.starts_with("--") {
            return Some((*token).to_string());
        }
    }
    None
}

fn flag_value(rest: &[&str], flag: &str) -> Option<String> {
    rest.iter()
        .position(|x| *x == flag)
        .and_then(|i| rest.get(i + 1))
        .map(|s| s.to_string())
}

fn flag_values(rest: &[&str], flag: &str) -> Vec<String> {
    rest.iter()
        .enumerate()
        .filter(|(_, value)| **value == flag)
        .filter_map(|(index, _)| rest.get(index + 1))
        .map(|value| (*value).to_string())
        .collect()
}

fn has_flag(rest: &[&str], flag: &str) -> bool {
    rest.contains(&flag)
}

pub(super) fn read_audit_events(limit: usize) -> Vec<Value> {
    let path =
        codeterm_service_core::plugin::audit::log_path(&codeterm_service_core::paths::data_dir());
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut events: Vec<Value> = text
        .lines()
        .rev()
        .take(limit)
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect();
    events.reverse();
    events
}

#[cfg(test)]
mod tests;
