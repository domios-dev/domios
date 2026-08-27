use crate::args::plugin::{PluginAction, PluginChannelAction};
use crate::{Cli, Commands};
use clap::Parser;

fn parse_plugin(args: &[&str]) -> PluginAction {
    let mut argv = vec!["codeterm", "plugin"];
    argv.extend_from_slice(args);
    match Cli::try_parse_from(argv)
        .expect("plugin args parse")
        .command
    {
        Commands::Plugin { action } => action,
        _ => panic!("expected plugin command"),
    }
}

#[test]
fn plugin_subcommands_parse() {
    match parse_plugin(&[
        "install",
        "--from-dir",
        "./provider-demo",
        "--id-override",
        "demo",
    ]) {
        PluginAction::Install {
            from_dir,
            id_override,
            ..
        } => {
            assert_eq!(from_dir.as_deref(), Some("./provider-demo"));
            assert_eq!(id_override.as_deref(), Some("demo"));
        }
        _ => panic!("expected plugin install"),
    }

    let cli = Cli::try_parse_from(["codeterm", "plugin", "list", "--json"])
        .expect("plugin list --json parses");
    assert!(cli.json);
    assert!(matches!(
        cli.command,
        Commands::Plugin {
            action: PluginAction::List
        }
    ));

    assert!(matches!(
        parse_plugin(&["update-system", "codex"]),
        PluginAction::UpdateSystem { id: Some(id) } if id == "codex"
    ));
    assert!(matches!(
        parse_plugin(&["system-update", "codex"]),
        PluginAction::UpdateSystem { id: Some(id) } if id == "codex"
    ));

    match parse_plugin(&[
        "channel",
        "add",
        "https://example.invalid/channel.git",
        "--name",
        "example",
    ]) {
        PluginAction::Channel {
            command: PluginChannelAction::Add { url, name },
        } => {
            assert_eq!(url, "https://example.invalid/channel.git");
            assert_eq!(name.as_deref(), Some("example"));
        }
        _ => panic!("expected plugin channel add"),
    }

    assert!(matches!(
        parse_plugin(&["channel", "diff", "example"]),
        PluginAction::Channel { command: PluginChannelAction::Diff { name: Some(name) } }
            if name == "example"
    ));

    assert!(matches!(
        parse_plugin(&["channel", "ensure-default"]),
        PluginAction::Channel {
            command: PluginChannelAction::EnsureDefault
        }
    ));

    assert!(matches!(
        parse_plugin(&["channel", "remove", "codeterm-core", "--force"]),
        PluginAction::Channel {
            command: PluginChannelAction::Remove { name, force: true }
        } if name == "codeterm-core"
    ));

    // `validate` must resolve as the built-in verb, not the `<id> <verb>` passthrough.
    assert!(matches!(
        parse_plugin(&["validate", "./provider-demo"]),
        PluginAction::Validate { dir } if dir == "./provider-demo"
    ));
}
