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
    let parts: Vec<&str> = args.iter().map(|a| a.as_ref()).collect();
    let action = parse_plugin_args(&parts)?;
    let ctx = PluginCtx {
        data_root: crate::paths::data_dir(),
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
    };
    codeterm_plugins::run(ctx, action)
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
            from_dir: None,
            id_override: None,
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
    rest.iter()
        .find(|x| !x.starts_with("--"))
        .map(|s| s.to_string())
}

fn flag_value(rest: &[&str], flag: &str) -> Option<String> {
    rest.iter()
        .position(|x| *x == flag)
        .and_then(|i| rest.get(i + 1))
        .map(|s| s.to_string())
}

fn has_flag(rest: &[&str], flag: &str) -> bool {
    rest.contains(&flag)
}

pub(super) fn read_audit_events(limit: usize) -> Vec<Value> {
    let path = crate::plugin::audit::log_path(&crate::paths::data_dir());
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
mod tests {
    use super::*;
    use codeterm_plugins::{PluginAction, PluginChannelAction};

    fn parse(args: &[&str]) -> PluginAction {
        parse_plugin_args(args).unwrap()
    }

    #[test]
    fn list_and_channel_reads() {
        assert!(matches!(
            parse(&["plugin", "list", "--json"]),
            PluginAction::List
        ));
        assert!(matches!(
            parse(&["plugin", "channel", "list", "--json"]),
            PluginAction::Channel(PluginChannelAction::List)
        ));
        assert!(matches!(
            parse(&["plugin", "channel", "ensure-default", "--json"]),
            PluginAction::Channel(PluginChannelAction::EnsureDefault)
        ));
        assert!(matches!(
            parse(&["plugin", "channel", "diff", "--json"]),
            PluginAction::Channel(PluginChannelAction::Diff { name: None })
        ));
    }

    #[test]
    fn install_with_channel() {
        match parse(&["plugin", "install", "demo", "--channel", "core", "--json"]) {
            PluginAction::Install { id, channel, .. } => {
                assert_eq!(id.as_deref(), Some("demo"));
                assert_eq!(channel.as_deref(), Some("core"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn update_optional_id() {
        match parse(&["plugin", "update", "--json"]) {
            PluginAction::Update { id } => assert!(id.is_none()),
            other => panic!("{other:?}"),
        }
        match parse(&["plugin", "update", "demo", "--json"]) {
            PluginAction::Update { id } => assert_eq!(id.as_deref(), Some("demo")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn remove_flags() {
        match parse(&[
            "plugin",
            "remove",
            "demo",
            "--force",
            "--remove-data",
            "--json",
        ]) {
            PluginAction::Remove {
                id,
                force,
                remove_data,
            } => {
                assert_eq!(id, "demo");
                assert!(force);
                assert!(remove_data);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn channel_add_and_remove() {
        match parse(&[
            "plugin",
            "channel",
            "add",
            "https://x/c.json",
            "--name",
            "demo",
            "--json",
        ]) {
            PluginAction::Channel(PluginChannelAction::Add { url, name }) => {
                assert_eq!(url, "https://x/c.json");
                assert_eq!(name.as_deref(), Some("demo"));
            }
            other => panic!("{other:?}"),
        }
        match parse(&["plugin", "channel", "remove", "demo", "--force", "--json"]) {
            PluginAction::Channel(PluginChannelAction::Remove { name, force }) => {
                assert_eq!(name, "demo");
                assert!(force);
            }
            other => panic!("{other:?}"),
        }
        match parse(&["plugin", "channel", "refresh", "demo", "--json"]) {
            PluginAction::Channel(PluginChannelAction::Refresh { name }) => {
                assert_eq!(name.as_deref(), Some("demo"))
            }
            other => panic!("{other:?}"),
        }
    }
}
