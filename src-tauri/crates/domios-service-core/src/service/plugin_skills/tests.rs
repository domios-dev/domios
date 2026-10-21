use rusqlite::Connection;

use codeterm_plugin::{js_runtime::PluginManifest, plugin_index::entries_from_manifest};

use crate::mem::{search, store, MemKind, MemScope, NewMemItem, SearchQuery};

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
        .contains("Invocation: `codeterm plugin telegram-client send <chat-id> <text>`"));
}
