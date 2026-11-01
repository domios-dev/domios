use rusqlite::Connection;

use codeterm_plugin::{js_runtime::PluginManifest, plugin_index::entries_from_manifest};

use crate::mem::{search, store, MemKind, MemScope, NewMemItem, SearchQuery};

#[test]
fn messaging_user_messages_recall_plugin_with_state_and_next_command() {
    use codeterm_plugin::js_runtime::catalog_from_manifest;
    use codeterm_plugin::plugin_connection::PluginConnectionState;
    use codeterm_plugin::plugin_index::entries_from_catalog;
    let conn = Connection::open_in_memory().unwrap();
    store::init_schema(&conn).unwrap();
    let blobs = tempfile::tempdir().unwrap();
    for (id, name, description, state) in [
        (
            "teams-client",
            "Teams Client",
            "Read Microsoft Teams chats and messages, find people and review conversations.",
            PluginConnectionState::SignedOut,
        ),
        (
            "telegram-client",
            "Telegram Client",
            "Read Telegram chats and messages, find people and review conversations.",
            PluginConnectionState::SignedIn,
        ),
        (
            "secret-tools",
            "Secret tools",
            "Authorize a secret for a person or pull request.",
            PluginConnectionState::Unknown,
        ),
    ] {
        let manifest = PluginManifest::from_json_str(
            &serde_json::json!({
                "id": id, "version":"1.0.0", "displayName": name, "description": description,
                "capabilities":{"agentProvider":false, "view":true},
                "agentVerbs":["login", "health", "chats [query]", "history <chat-id>"]
            })
            .to_string(),
        )
        .unwrap();
        let mut catalog = catalog_from_manifest(&manifest, false);
        catalog.connection_state = Some(state);
        for entry in entries_from_catalog(&manifest, None, None, &catalog) {
            store::insert(&conn, &super::to_new_item(entry), blobs.path()).unwrap();
        }
    }
    for (query, plugin, state, command) in [
        (
            "Anna wrote to me in Teams, check the PRs",
            "teams-client",
            "not signed in",
            "login",
        ),
        (
            "Anna sent a Telegram message about the PR",
            "telegram-client",
            "signed in",
            "--help",
        ),
        ("Teams", "teams-client", "not signed in", "login"),
        ("Telegram", "telegram-client", "signed in", "--help"),
    ] {
        let hits = search::run(
            &conn,
            &SearchQuery {
                query: query.into(),
                limit: Some(3),
                ..Default::default()
            },
            blobs.path(),
        )
        .unwrap();
        let hit = hits
            .iter()
            .find(|hit| {
                hit.item.source.as_deref().is_some_and(|source| {
                    source.starts_with(&format!("plugin-agent-verbs:{plugin}"))
                })
            })
            .expect("matching messaging plugin must rank in the top three");
        let snippet: String = hit
            .item
            .body
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(139)
            .collect();
        assert!(snippet.contains(state), "{snippet}");
        assert!(
            snippet.contains(&format!("`domios plugin {plugin} {command}`")),
            "{snippet}"
        );
        assert!(!hit.item.body.contains("private-code"));
    }
}

#[test]
fn index_sync_makes_zero_plugin_calls_and_tracks_login_upgrade_and_uninstall() {
    use codeterm_plugin::agent_commands::AgentCommandProvider;
    use codeterm_plugin::js_runtime::{catalog_from_manifest, JsAgentCommandHandler, PluginVm};
    use codeterm_plugin::plugin_index::{plan_plugin_index_sync, ExistingPluginIndexEntry};
    use std::sync::Arc;
    let root = tempfile::tempdir().unwrap();
    let plugin_dir = root.path().join("opaque-client");
    std::fs::create_dir(&plugin_dir).unwrap();
    let path = plugin_dir.join("plugin.json");
    let manifest_json = serde_json::json!({
        "id":"opaque-client", "version":"1.0.0", "displayName":"Teams Fixture",
        "description":"Read Teams messages", "agentVerbs":["health", "login", "login-status", "logout"],
        "capabilities":{"agentProvider":false}
    });
    std::fs::write(&path, manifest_json.to_string()).unwrap();
    let manifest = PluginManifest::load(&path).unwrap();
    let vm = Arc::new(PluginVm::from_source("opaque-client", r#"
        let signedIn = false;
        let commandCalls = 0;
        module.exports = {onAgentCommand(ctx) {
            commandCalls++;
            if (ctx.verb === 'login') signedIn = true;
            return {result: JSON.stringify({state:signedIn?'logged-in':'logged-out', deviceCode:'private-code'})};
        }, commandCalls() {return commandCalls;}};
    "#.into()).unwrap());
    let handler = Arc::new(
        JsAgentCommandHandler::new(vm.clone(), "opaque-client", true)
            .with_catalog(catalog_from_manifest(&manifest, false)),
    );
    let registry = super::AgentCommandRegistry::new();
    registry.register("opaque-client", handler.clone());
    let mem = crate::mem::MemManager::open_in_memory().unwrap();
    super::sync_at(&mem, &registry, root.path());
    super::sync_at(&mem, &registry, root.path());
    assert_eq!(
        codeterm_plugin::js_runtime::call_json::<usize>(&vm, "commandCalls", &[]).unwrap(),
        0
    );
    let before = super::installed_plugin_entries_at(root.path(), &registry).unwrap();
    assert_eq!(before.len(), 5);
    assert!(before.iter().all(|entry| entry.body.starts_with(
        "Teams Fixture: state unknown — run `domios plugin opaque-client login-status`"
    )));
    let persisted = mem
        .list_by_source_prefix("skill", "plugin-agent-verbs:")
        .unwrap();
    assert_eq!(persisted.len(), 5);
    assert!(persisted
        .iter()
        .all(|entry| entry.body.contains("state unknown")));
    let existing: Vec<_> = before
        .iter()
        .map(|entry| ExistingPluginIndexEntry {
            mem_id: entry.title.clone(),
            title: entry.title.clone(),
            source: entry.source.clone(),
            body: entry.body.clone(),
        })
        .collect();
    handler.on_agent_command("tab", "login", &[], None).unwrap();
    super::sync_at(&mem, &registry, root.path());
    let persisted = mem
        .list_by_source_prefix("skill", "plugin-agent-verbs:")
        .unwrap();
    assert_eq!(persisted.len(), 5);
    assert!(persisted
        .iter()
        .all(|entry| entry.body.starts_with("Teams Fixture: signed in.")));
    assert_eq!(
        codeterm_plugin::js_runtime::call_json::<usize>(&vm, "commandCalls", &[]).unwrap(),
        1
    );
    let after = super::installed_plugin_entries_at(root.path(), &registry).unwrap();
    assert!(after
        .iter()
        .all(|entry| entry.body.starts_with("Teams Fixture: signed in.")));
    assert!(after
        .iter()
        .all(|entry| !entry.body.contains("private-code")));
    assert_eq!(
        plan_plugin_index_sync(&after, &existing).upserts.len(),
        after.len()
    );
    assert!(plan_plugin_index_sync(&after, &existing)
        .removals
        .is_empty());
    let mut upgraded = manifest_json;
    upgraded["version"] = "2.0.0".into();
    upgraded["description"] = "Read Teams messages and files".into();
    std::fs::write(&path, upgraded.to_string()).unwrap();
    let upgraded = super::installed_plugin_entries_at(root.path(), &registry).unwrap();
    assert!(upgraded[0].body.contains("Read Teams messages and files"));
    std::fs::remove_file(path).unwrap();
    super::sync_at(&mem, &registry, root.path());
    assert!(mem
        .list_by_source_prefix("skill", "plugin-agent-verbs:")
        .unwrap()
        .is_empty());
    assert_eq!(
        codeterm_plugin::js_runtime::call_json::<usize>(&vm, "commandCalls", &[]).unwrap(),
        1
    );
    let removed = super::installed_plugin_entries_at(root.path(), &registry).unwrap();
    assert!(removed.is_empty());
    assert_eq!(
        plan_plugin_index_sync(&removed, &existing).removals.len(),
        existing.len()
    );
}

#[test]
fn fixture_mem_search_ranks_the_matching_plugin_verb() {
    let manifest = PluginManifest::from_json_str(
        r#"{
            "id": "telegram-client",
            "version": "1.0.0",
            "displayName": "Telegram Client",
            "description": "Manage Telegram chat connections.",
            "agentVerbs": ["send <chat-id> <text>", "status <chat-id>"],
            "agentKeywords": ["telegram", "message", "chat"]
        }"#,
    )
    .unwrap();
    let guide = "## send\nSend a Telegram message to the selected chat.\n\n## status\nShow the Telegram connection state.";
    let entries = entries_from_manifest(&manifest, None, Some(guide));
    let conn = Connection::open_in_memory().unwrap();
    store::init_schema(&conn).unwrap();
    let blobs = tempfile::tempdir().unwrap();
    for entry in entries {
        store::insert(
            &conn,
            &NewMemItem {
                kind: MemKind::Skill,
                scope: MemScope::Global,
                title: entry.title,
                summary: Some(entry.summary),
                area: None,
                tags: entry.tags,
                body: entry.body,
                source: Some(entry.source),
                importance: 6.0,
                source_path: None,
                pinned: false,
            },
            blobs.path(),
        )
        .unwrap();
    }

    let hits = search::run(
        &conn,
        &SearchQuery {
            query: "send Telegram message".into(),
            limit: Some(5),
            ..Default::default()
        },
        blobs.path(),
    )
    .unwrap();

    let best = hits.first().expect("plugin verb result");
    assert!(best.item.title.contains("send-chat-id-text"));
    assert!(best
        .item
        .body
        .contains("Invocation: `domios plugin telegram-client send <chat-id> <text>`"));
}
