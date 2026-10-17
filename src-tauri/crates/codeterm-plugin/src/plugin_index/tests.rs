use serde_json::json;

use super::*;

fn manifest() -> PluginManifest {
    PluginManifest::from_json_str(
        r#"{
            "id": "telegram-client",
            "version": "1.0.0",
            "displayName": "Telegram Client",
            "description": "Bridge Telegram chats and message delivery.",
            "agentVerbs": ["send <chat-id> <text>", "status <chat-id>"],
            "agentKeywords": ["telegram", "message", "chat"],
            "capabilities": {
                "agentProvider": false,
                "view": true,
                "surfaces": [
                    { "id": "messages", "title": "Messages", "renderer": "view" }
                ]
            }
        }"#,
    )
    .unwrap()
}

fn settings_schema() -> Value {
    json!([
        {
            "kind": "section",
            "title": "Telegram",
            "description": "Connect an account.",
            "fields": [
                {
                    "kind": "toggle",
                    "key": "sendReceipts",
                    "label": "Send receipts",
                    "description": "Deliver a receipt after each message."
                }
            ]
        }
    ])
}

fn entries() -> Vec<PluginIndexEntry> {
    entries_from_manifest(
        &manifest(),
        Some(&settings_schema()),
        Some("## send\nSend a Telegram message to the selected chat."),
    )
}

fn stored(entry: &PluginIndexEntry, mem_id: &str) -> ExistingPluginIndexEntry {
    ExistingPluginIndexEntry {
        mem_id: mem_id.to_string(),
        title: entry.title.clone(),
        source: entry.source.clone(),
        body: entry.body.clone(),
    }
}

#[test]
fn manifest_metadata_becomes_a_plugin_entry_and_one_entry_per_verb() {
    let entries = entries();
    assert_eq!(entries.len(), 3);
    let plugin = entries
        .iter()
        .find(|entry| entry.source.ends_with("telegram-client"))
        .unwrap();
    assert!(plugin.body.contains("agent provider disabled"));
    assert!(plugin.body.contains("chat backend") || plugin.body.contains("view enabled"));
    assert!(plugin.body.contains("messages (Messages, view)"));
    assert!(plugin
        .body
        .contains("sendReceipts: Send receipts: Deliver a receipt after each message."));
    assert!(plugin.body.contains("codeterm plugin get telegram-client"));

    let send = entries
        .iter()
        .find(|entry| entry.source.ends_with("verb:send-chat-id-text"))
        .unwrap();
    assert!(send.title.contains("send-chat-id-text"));
    assert!(send.body.contains("Arguments: `<chat-id> <text>`"));
    assert!(send
        .body
        .contains("Send a Telegram message to the selected chat."));
    assert!(send
        .body
        .contains("Invocation: `codeterm plugin telegram-client send <chat-id> <text>`"));
    assert!(send.tags.contains(&"telegram".to_string()));
}

#[test]
fn uninstall_reconciliation_removes_plugin_and_verb_entries() {
    let gone = entries();
    let kept = entries_from_manifest(
        &PluginManifest::from_json_str(
            r#"{"id":"kept-plugin","version":"1.0.0","displayName":"Kept"}"#,
        )
        .unwrap(),
        None,
        None,
    );
    let existing = gone
        .iter()
        .enumerate()
        .map(|(index, entry)| stored(entry, &format!("gone-{index}")))
        .chain(
            kept.iter()
                .map(|entry| stored(entry, &format!("kept-{}", entry.title))),
        )
        .collect::<Vec<_>>();

    let plan = plan_plugin_index_sync(&kept, &existing);

    assert_eq!(plan.removals.len(), gone.len());
    assert!(plan.removals.iter().all(|id| id.starts_with("gone-")));
    assert!(plan.upserts.is_empty());
}

#[test]
fn changed_manifest_replaces_only_the_changed_entry() {
    let old = entries();
    let mut changed_manifest = manifest();
    changed_manifest.description = Some("Bridge Telegram files.".into());
    let changed = entries_from_manifest(&changed_manifest, Some(&settings_schema()), None);
    let existing = old
        .iter()
        .map(|entry| stored(entry, &entry.title))
        .collect::<Vec<_>>();

    let plan = plan_plugin_index_sync(&changed, &existing);

    assert_eq!(plan.upserts.len(), 3);
    assert!(plan.removals.is_empty());
}

#[test]
fn distinct_verbs_with_the_same_search_slug_keep_separate_entries() {
    let manifest = PluginManifest::from_json_str(
        r#"{
            "id": "file-tools",
            "version": "1.0.0",
            "displayName": "File Tools",
            "agentVerbs": ["run <file-path>", "run-file <path>"]
        }"#,
    )
    .unwrap();

    let entries = entries_from_manifest(&manifest, None, None);
    let verbs = entries
        .iter()
        .filter(|entry| entry.source.contains(":verb:"))
        .collect::<Vec<_>>();

    assert_eq!(verbs.len(), 2);
    assert_ne!(verbs[0].source, verbs[1].source);
    assert_ne!(verbs[0].title, verbs[1].title);
}
