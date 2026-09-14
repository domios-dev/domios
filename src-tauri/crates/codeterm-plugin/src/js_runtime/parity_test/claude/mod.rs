//! Claude provider parity — native `ClaudeProvider` vs the bundled
//! `provider-claude` JS plugin. Shared helpers live here; the cases are
//! split across submodules to keep each file under the size limit.

use super::*;
pub(crate) use crate::agent_claude::provider::ClaudeProvider;

mod catalog;
mod commands;
mod core;
mod launch_session;
mod permission_request;
mod sessions;

fn pair() -> (ClaudeProvider, JsProvider) {
    (ClaudeProvider::new(), load_js("claude"))
}

/// Synthetic Claude plugin variant that points its rule + integration
/// paths into a tempdir so we exercise the real plugin.js end-to-end
/// against `JsProvider` without touching the user's real `~/.claude/`.
fn claude_in_temp(rule_path: &std::path::Path, settings_path: &std::path::Path) -> JsProvider {
    let plugin_js = std::fs::read_to_string(
        workspace_plugins_root()
            .join("provider-claude")
            .join("plugin.js"),
    )
    .unwrap();
    let manifest_json = format!(
        r#"{{
                "id":"claude-test","version":"0.1.0","displayName":"X",
                "spawn":{{"readinessStrategy":{{"kind":"fixed_delay","durationMs":0}},
                         "taskDelivery":"pty_input",
                         "verifySubmit":{{"kind":"none"}}}},
                "rule":{{"path":"{rp}"}},
                "integration":{{"kind":"claude_settings_json","settingsPath":"{sp}"}}
            }}"#,
        rp = rule_path.to_string_lossy().replace('\\', "/"),
        sp = settings_path.to_string_lossy().replace('\\', "/"),
    );
    let manifest: crate::js_runtime::PluginManifest = serde_json::from_str(&manifest_json).unwrap();
    let vm = crate::js_runtime::vm::PluginVm::from_source_with(
        "claude-test",
        plugin_js,
        Some(manifest_json),
    )
    .unwrap();
    JsProvider::new(manifest, std::sync::Arc::new(vm))
}

/// A JS provider that reimplements AskUserQuestion extraction in JS,
/// declaring `interactiveEvents: true` so the host dispatches to it.
fn claude_interactive_js() -> JsProvider {
    let manifest_json = r#"{
            "id":"claude-interactive","version":"0.1.0","displayName":"X",
            "spawn":{"readinessStrategy":{"kind":"fixed_delay","durationMs":0},
                     "taskDelivery":"pty_input",
                     "verifySubmit":{"kind":"none"},
                     "interactiveEvents":true}
        }"#;
    let plugin_js = r#"
            module.exports.default = {
                parseInteractiveEvents(msg) {
                    var out = [];
                    var blocks = (msg && msg.blocks) || [];
                    for (var bi = 0; bi < blocks.length; bi++) {
                        var b = blocks[bi];
                        if (!b || b.kind !== "tool_use") continue;
                        var data = b.data || {};
                        if (data.name !== "AskUserQuestion") continue;
                        var toolId = data.id || "";
                        var input = data.input || {};
                        var questions = input.questions;
                        if (!Array.isArray(questions)) continue;
                        for (var i = 0; i < questions.length; i++) {
                            var q = questions[i];
                            if (!q || !Array.isArray(q.options)) continue;
                            var options = [];
                            for (var oi = 0; oi < q.options.length; oi++) {
                                var o = q.options[oi];
                                if (!o || typeof o.label !== "string") continue;
                                var opt = { label: o.label };
                                if (typeof o.description === "string") opt.description = o.description;
                                options.push(opt);
                            }
                            var elem = {
                                type: "question",
                                id: toolId + "-" + i,
                                toolUseId: toolId,
                                prompt: typeof q.question === "string" ? q.question : "",
                                options: options,
                                multiSelect: q.multiSelect === true
                            };
                            if (typeof q.header === "string") elem.header = q.header;
                            out.push(elem);
                        }
                    }
                    return out;
                }
            };
        "#;
    let manifest: crate::js_runtime::PluginManifest = serde_json::from_str(manifest_json).unwrap();
    let vm = crate::js_runtime::vm::PluginVm::from_source_with(
        "claude-interactive",
        plugin_js.to_string(),
        Some(manifest_json.to_string()),
    )
    .unwrap();
    JsProvider::new(manifest, std::sync::Arc::new(vm))
}

fn ask_user_question_fixture() -> codeterm_types::agent::chat_types::RichMessage {
    use codeterm_types::agent::chat_types::{MessageBlock, RichMessage};
    use serde_json::json;
    RichMessage {
        role: "assistant".into(),
        blocks: vec![
            MessageBlock {
                kind: "text".into(),
                data: json!({"text": "deciding"}),
            },
            MessageBlock {
                kind: "tool_use".into(),
                data: json!({
                    "id": "toolu_parity",
                    "name": "AskUserQuestion",
                    "input": {
                        "questions": [
                            {
                                "question": "Pick a runtime",
                                "header": "Runtime",
                                "multiSelect": false,
                                "options": [
                                    {"label": "Node", "description": "JS runtime"},
                                    {"label": "Deno"}
                                ]
                            },
                            {
                                "question": "Pick features",
                                "multiSelect": true,
                                "options": [
                                    {"label": "TLS"},
                                    {"label": "HTTP2", "description": "multiplexed"}
                                ]
                            }
                        ]
                    }
                }),
            },
        ],
        ..Default::default()
    }
}
