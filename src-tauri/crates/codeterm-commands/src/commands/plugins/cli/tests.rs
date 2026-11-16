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
fn directory_install_preserves_source_and_optional_identity() {
    match parse(&[
        "plugin",
        "install",
        "--from-dir",
        "C:/local plugin",
        "--id-override",
        "renamed",
        "--json",
    ]) {
        PluginAction::Install {
            id,
            from_dir,
            id_override,
            channel,
        } => {
            assert_eq!(id, None);
            assert_eq!(from_dir.as_deref(), Some("C:/local plugin"));
            assert_eq!(id_override.as_deref(), Some("renamed"));
            assert_eq!(channel, None);
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
fn settings_parses_repeated_host_grants() {
    match parse(&[
        "plugin",
        "settings",
        "demo",
        "--allow-host",
        "api.example.test",
        "--allow-host",
        "api.example.test:8443",
        "--disallow-host",
        "old.example.test",
        "--json",
    ]) {
        PluginAction::Settings {
            id,
            allow_host,
            disallow_host,
        } => {
            assert_eq!(id, "demo");
            assert_eq!(allow_host, ["api.example.test", "api.example.test:8443"]);
            assert_eq!(disallow_host, ["old.example.test"]);
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
