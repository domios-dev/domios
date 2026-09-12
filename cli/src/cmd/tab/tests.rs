use super::*;
use crate::client::SocketCallError;
use serde_json::json;

#[derive(Default)]
struct SearchRpc {
    calls: Vec<(String, Option<serde_json::Value>)>,
    tabs: Vec<serde_json::Value>,
}

impl RpcClient for SearchRpc {
    fn call_detailed(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, SocketCallError> {
        self.calls.push((method.to_string(), params));
        Ok(serde_json::Value::Array(self.tabs.clone()))
    }
}

#[test]
fn tab_list_request_scopes_to_calling_tab_when_workspace_env_is_absent() {
    let params = tab_list_params(None, false, false, Some("self-tab".into()));

    assert_eq!(params["caller_tab_id"], "self-tab");
    assert_eq!(params["focused_workspace"], true);
}

#[test]
fn tab_list_request_drops_calling_tab_for_explicit_scope() {
    let workspace = tab_list_params(
        Some("workspace".into()),
        false,
        false,
        Some("self-tab".into()),
    );
    let all = tab_list_params(None, true, false, Some("self-tab".into()));

    assert!(workspace["caller_tab_id"].is_null());
    assert!(all["caller_tab_id"].is_null());
}

#[test]
fn explicit_address_scope_tab_search_sees_foreign_workspace_titles_and_ids() {
    let foreign = json!({
        "id": "deadbeef1234",
        "tab_title": "Foreign Dalton",
        "workspace_name": "Elsewhere",
        "workspace_id": "ws_foreign"
    });
    let mut client = SearchRpc {
        tabs: vec![foreign],
        ..Default::default()
    };

    let response = fetch_search_tabs(&mut client).unwrap();
    assert_eq!(client.calls[0].0, "tab.list");
    let params = client.calls[0].1.as_ref().expect("tab.list params");
    assert_eq!(params["all_workspaces"], true);
    assert_eq!(params["focused_workspace"], false);
    assert!(params["caller_tab_id"].is_null());

    let tabs = response.as_array().expect("tab.list array");
    assert_eq!(rank_ids("Dalton", &tabs), vec!["deadbeef1234".to_string()]);
    assert_eq!(
        rank_ids("deadbeef", &tabs),
        vec!["deadbeef1234".to_string()]
    );
    assert!(format_tab_row(&tabs[0]).contains("ws=Elsewhere"));
}

fn make_tab(id: &str, title: &str, agent: Option<&str>, sname: Option<&str>) -> serde_json::Value {
    let mut p = json!({"id": id, "title": title});
    if let Some(a) = agent {
        p["agent_type"] = json!(a);
    }
    if let Some(n) = sname {
        p["agent_session_name"] = json!(n);
    }
    p
}

fn rank_ids(needle: &str, tabs: &[serde_json::Value]) -> Vec<String> {
    rank_tabs(needle, tabs)
        .into_iter()
        .map(|(_, p)| {
            p.get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

fn string_field<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(|field| field.as_str())
        .unwrap_or("")
}

#[test]
fn render_tree_groups_by_workspace_and_nests_children() {
    let tabs = vec![
        json!({"id":"aaaa","tab_title":"MainX","agent_session_model":"Sonnet","workspace_name":"General","workspace_id":"ws1"}),
        json!({"id":"bbbb","tab_title":"Torvalds","agent_session_model":"Opus","workspace_name":"General","workspace_id":"ws1"}),
        json!({"id":"cccc","tab_title":"scout","agent_session_model":"Sonnet","workspace_name":"General","workspace_id":"ws1","parent_tab_id":"bbbb"}),
        json!({"id":"dddd","tab_title":"devbox","agent_session_model":"Sonnet","workspace_name":"codeterm","workspace_id":"ws2"}),
    ];
    let out = render_tab_tree(&tabs);
    assert!(
        out.contains("General  ws1"),
        "workspace header shows name + ws id:\n{}",
        out
    );
    assert!(
        out.contains("codeterm  ws2"),
        "second workspace header:\n{}",
        out
    );

    let lines: Vec<&str> = out.lines().collect();
    let torvalds = lines.iter().position(|l| l.contains("Torvalds")).unwrap();
    let scout = lines.iter().position(|l| l.contains("scout")).unwrap();
    assert_eq!(
        scout,
        torvalds + 1,
        "child renders directly under its parent:\n{}",
        out
    );
    let indent = |l: &str| l.len() - l.trim_start().len();
    assert!(
        indent(lines[scout]) > indent(lines[torvalds]),
        "child indented deeper:\n{}",
        out
    );
    assert!(
        lines[torvalds].trim_start().starts_with("bbbb"),
        "id comes first: {}",
        lines[torvalds]
    );
}

#[test]
fn tab_list_uses_ordinary_remote_projection_without_remote_worker_ghosts() {
    let tabs = vec![
        json!({
            "id":"shared-tab", "title":"Local", "workspace_name":"CodeTerm", "workspace_id":"ws"
        }),
        json!({
            "id":"orch", "title":"Dumas", "role":"Orchestrator", "workspace_name":"CodeTerm", "workspace_id":"ws"
        }),
        json!({
            "id":"mesh:peer-a/shared-tab", "remote_tab_id":"shared-tab", "title":"A", "role":"Manager", "role_profile":"planner", "remote_peer_name":"A"
        }),
        json!({
            "id":"mesh:peer-b/shared-tab", "remote_tab_id":"shared-tab", "title":"B", "role":"Orchestrator", "remote_peer_name":"B"
        }),
    ];

    let ids: Vec<_> = tabs.iter().map(|tab| string_field(tab, "id")).collect();
    assert_eq!(
        ids,
        [
            "shared-tab",
            "orch",
            "mesh:peer-a/shared-tab",
            "mesh:peer-b/shared-tab"
        ]
    );
    assert_eq!(string_field(&tabs[2], "remote_tab_id"), "shared-tab");
    assert_eq!(string_field(&tabs[2], "role_profile"), "planner");
    assert_eq!(string_field(&tabs[2], "remote_peer_name"), "A");
    assert_eq!(
        ordinary_tab_list_path(&tab_list_params(None, true, false, None)),
        "/api/tabs?all_workspaces=true"
    );
}

#[test]
fn render_tree_falls_back_to_agent_type_without_model() {
    let tabs = vec![
        json!({"id":"eeee","tab_title":"plain","workspace_name":"W","workspace_id":"w"}),
        json!({"id":"ffff","tab_title":"coder","agent_type":"claude","workspace_name":"W","workspace_id":"w"}),
    ];
    let out = render_tab_tree(&tabs);
    // No model + no agent_type → bare id + name (no trailing junk).
    assert!(
        out.contains("eeee plain\n"),
        "plain Tab is id + name only:\n{}",
        out
    );
    // agent_type fills in when the session model is absent.
    assert!(
        out.contains("claude"),
        "agent_type shown when model missing:\n{}",
        out
    );
}

#[test]
fn render_tree_places_team_terminals_after_workers_with_creator_provenance() {
    let tabs = vec![
        json!({"id":"orch","tab_title":"Ada","role":"Orchestrator","team_id":"team","workspace_name":"W","workspace_id":"w"}),
        json!({"id":"terminal","tab_title":"dev server","team_id":"team","spawned_by_tab_id":"orch","parent_tab_id":"orch","workspace_name":"W","workspace_id":"w"}),
        json!({"id":"worker","tab_title":"Worker","role":"Worker","team_id":"team","parent_tab_id":"orch","workspace_name":"W","workspace_id":"w"}),
    ];

    let out = render_tab_tree(&tabs);
    let worker = out.find("worker Worker").unwrap();
    let terminal = out.find("terminal dev server").unwrap();
    assert!(
        worker < terminal,
        "Team terminal must follow workers:\n{out}"
    );
    assert!(
        out.contains("\u{b7}terminal by Ada"),
        "creator provenance missing:\n{out}"
    );
}

/// One bracket rule across every row the tree renders: the bracket is the
/// agent identity, role rides its own suffix, and liveness never reads as live
/// unless it was observed.
#[test]
fn render_tree_brackets_the_agent_identity_and_never_the_role() {
    let tabs = vec![
        json!({"id":"orch","tab_title":"Ada","role":"Manager","role_profile":"orchestrator",
               "agent_type":"claude","agent_session_model":"opus","agent_liveness":"live",
               "workspace_name":"W","workspace_id":"w"}),
        json!({"id":"gen","tab_title":"Main","role":"Manager","role_profile":"general_agent",
               "agent_type":"claude","agent_session_model":"opus","agent_liveness":"live",
               "workspace_name":"W","workspace_id":"w"}),
        json!({"id":"gone","tab_title":"Ghost","role":"Worker",
               "agent_type":"codex","agent_session_model":"gpt-5.6-sol","agent_liveness":"absent",
               "workspace_name":"W","workspace_id":"w"}),
    ];
    let out = render_tab_tree(&tabs);

    assert!(
        out.contains("[opus]"),
        "orchestrator bracket:\n{out}"
    );
    assert!(
        out.contains("[sol!]"),
        "a tab with no live agent is marked, not shown as running:\n{out}"
    );
    assert!(
        !out.contains("[manager]") && !out.contains("[general_agent]") && !out.contains("[worker]"),
        "role must not reach the bracket:\n{out}"
    );
    // The General Agent and the orchestrator render the same bracket; only the
    // role suffix separates them.
    assert!(out.contains("\u{b7}general_agent"), "role suffix:\n{out}");
    assert!(out.contains("\u{b7}orchestrator"), "role suffix:\n{out}");
}

#[test]
fn render_tree_includes_description() {
    let tabs = vec![json!({
        "id":"eeee",
        "tab_title":"plain",
        "description":"running tests",
        "workspace_name":"W",
        "workspace_id":"w"
    })];
    let out = render_tab_tree(&tabs);
    assert!(
        out.contains("eeee plain - running tests\n"),
        "description missing from Tab list:\n{}",
        out
    );
}

#[test]
fn render_tree_marks_only_the_calling_tab() {
    let tabs = vec![
        json!({"id":"self01","tab_title":"current","workspace_name":"W","workspace_id":"w"}),
        json!({"id":"peer02","tab_title":"peer","workspace_name":"W","workspace_id":"w"}),
    ];
    let out = render_tab_tree_with_current(&tabs, Some("self01"));
    assert!(out.contains("self01 current (self)\n"));
    assert!(!out.contains("peer02 peer (self)"));
}

#[test]
fn short_needle_does_not_match_id_prefix() {
    let tabs = vec![
        make_tab("aaaaa1", "OtherTab", Some("claude"), None),
        make_tab("bbbb22", "a", None, None),
    ];
    let ranked = rank_ids("a", &tabs);
    assert_eq!(
        ranked.first().map(String::as_str),
        Some("bbbb22"),
        "exact title match must beat id-prefix when needle is short"
    );
}

#[test]
fn long_needle_id_prefix_still_wins() {
    let tabs = vec![
        make_tab("abcd1234", "ignored", None, None),
        make_tab("ffffffff", "abcd", None, None),
    ];
    let ranked = rank_ids("abcd", &tabs);
    assert_eq!(
        ranked.first().map(String::as_str),
        Some("abcd1234"),
        "id-prefix dominates at needle length ≥ 4"
    );
}

#[test]
fn tie_break_is_stable_by_id() {
    let tabs = vec![
        make_tab("zzzz9999", "foo-bar", None, None),
        make_tab("aaaa1111", "foo-baz", None, None),
    ];
    let ranked = rank_ids("foo", &tabs);
    assert_eq!(ranked, vec!["aaaa1111".to_string(), "zzzz9999".to_string()]);
}

#[test]
fn tab_title_matches_friendly_name() {
    // Regression: use workspace-tree tab title in matching so labels like "dalton" still find the expected tab.
    let tabs = vec![
        json!({"id": "11111111", "title": "claude-with-text", "tab_title": "Dalton"}),
        json!({"id": "22222222", "title": "silent-claude",    "tab_title": "Thomson"}),
    ];
    let ranked = rank_ids("dalton", &tabs);
    assert_eq!(
        ranked.first().map(String::as_str),
        Some("11111111"),
        "tab_title must be searched — that's the name the user sees"
    );
}

#[test]
fn tab_title_contains_outranks_title_contains() {
    // Tab title is more user-facing than the raw tab.title; "fib" should hit Fibonacci's tab over a tab that just has "fib" in its title.
    let tabs = vec![
        json!({"id": "aa11", "title": "ignore-me-fib"}),
        json!({"id": "bb22", "title": "claude", "tab_title": "Fibonacci"}),
    ];
    let ranked = rank_ids("fib", &tabs);
    assert_eq!(ranked.first().map(String::as_str), Some("bb22"));
}

#[test]
fn model_field_is_searchable() {
    // Asking for "sonnet" or "haiku" finds tabs currently running that model — covers the "which tabs have opus available?" workflow.
    let tabs = vec![
        json!({"id": "p1111111", "title": "t1", "agent_type": "claude",  "agent_session_model": "Sonnet"}),
        json!({"id": "p2222222", "title": "t2", "agent_type": "opencode","agent_session_model": "Deepseek"}),
    ];
    let ranked = rank_ids("sonnet", &tabs);
    assert_eq!(ranked.first().map(String::as_str), Some("p1111111"));
}

#[test]
fn human_row_shows_tab_title_with_model() {
    let p = json!({
        "id": "abcd1234",
        "title": "claude-with-text",
        "tab_title": "Dalton",
        "agent_type": "claude",
        "agent_session_model": "Sonnet",
        "workspace_name": "WS-claude-renamed",
    });
    let row = format_tab_row(&p);
    assert!(row.contains("Dalton"), "row missing tab_title: {}", row);
    // One label only: tab_title wins, so the raw tab.title must NOT leak in.
    assert!(
        !row.contains("claude-with-text"),
        "tab_title should win over tab.title: {}",
        row
    );
    assert!(row.contains("claude"), "row missing agent: {}", row);
    assert!(row.contains("Sonnet"), "row missing model: {}", row);
    assert!(
        row.contains("ws=WS-claude-renamed"),
        "row missing workspace: {}",
        row
    );
}

#[test]
fn cyrillic_needle_matches_latin_tab_title() {
    // User types tab name in Russian — should still find the Dalton tab.
    let tabs = vec![
        json!({"id": "11111111", "title": "claude", "tab_title": "Dalton"}),
        json!({"id": "22222222", "title": "claude", "tab_title": "Thomson"}),
    ];
    let ranked = rank_ids("Далтон", &tabs);
    assert_eq!(
        ranked.first().map(String::as_str),
        Some("11111111"),
        "cyrillic 'Далтон' must transliterate to 'Dalton' and match"
    );
}

#[test]
fn latin_needle_matches_cyrillic_tab_title() {
    // Reverse matching uses deunicode romanization so Cyrillic tab names can be found by Latin input.
    let tabs = vec![
        json!({"id": "33333333", "title": "claude", "tab_title": "Андрей"}),
        json!({"id": "44444444", "title": "claude", "tab_title": "Misha"}),
    ];
    let ranked = rank_ids("andrei", &tabs);
    assert_eq!(
        ranked.first().map(String::as_str),
        Some("33333333"),
        "latin 'andrei' must match the transliterated form of 'Андрей'"
    );
}

#[test]
fn human_row_collapses_when_tab_title_equals_tab_title() {
    let p = json!({
        "id": "abcd1234",
        "title": "Main",
        "tab_title": "Main",
        "agent_type": "claude",
    });
    let row = format_tab_row(&p);
    // No "(Main)" duplicate — single label only.
    assert!(!row.contains("(Main)"), "duplicate label in row: {}", row);
}

#[test]
fn inspect_rejects_mixed_and_multi_peer_owners() {
    let mixed = vec!["local".into(), "mesh:peer/remote".into()];
    assert!(super::remote_inspect_target(&mixed)
        .unwrap_err()
        .contains("mixed"));
    let peers = vec!["mesh:a/one".into(), "mesh:b/two".into()];
    assert!(super::remote_inspect_target(&peers)
        .unwrap_err()
        .contains("multi-peer"));
}

#[test]
fn composite_parser_rejects_rest_and_whitespace_delimiters() {
    for id in ["mesh:p/a b", "mesh:p/a?b", "mesh:p/a&b", "mesh:p/a#b"] {
        assert!(super::parse_remote_tab_id(id).is_none(), "accepted {id}");
    }
}

#[test]
fn malformed_remote_targets_never_classify_as_local() {
    for id in [
        "mesh:p/a b",
        "mesh:p/a?b",
        "mesh:p/a&b",
        "mesh:p/a#b",
        "mesh:/tab",
    ] {
        assert_eq!(
            super::classify_remote_tab_id(id),
            super::RemoteTabTarget::Invalid
        );
    }
    assert_eq!(
        super::classify_remote_tab_id("local"),
        super::RemoteTabTarget::Local
    );
}

#[test]
fn every_tab_action_rejects_malformed_remote_before_local_transport() {
    use crate::args::misc::TabCaptureAction;
    let bad = || Some("mesh:peer/bad?tab".to_string());
    let actions = vec![
        TabAction::Inspect {
            tabs: vec![bad().unwrap()],
            tab: vec![],
            lifecycle: false,
        },
        TabAction::Focus {
            id: bad(),
            tab: None,
        },
        TabAction::Close {
            id: bad(),
            tab: None,
        },
        TabAction::Rename {
            tab: bad(),
            title: "x".into(),
        },
        TabAction::Describe {
            tab: bad(),
            summary: "x".into(),
        },
        TabAction::GetText {
            tab: bad(),
            formatted: false,
        },
        TabAction::Exec {
            tab: bad(),
            command: "x".into(),
            timeout: 1,
        },
        TabAction::Status { tab: bad() },
        TabAction::Respond {
            option: 1,
            tab: bad(),
        },
        TabAction::Rebind {
            session_id: "session-live".into(),
            tab: bad(),
        },
        TabAction::Repair {
            provider: None,
            session: None,
            from_evidence: true,
            from_journal: false,
            tab: bad(),
        },
        TabAction::Stop { tab: bad() },
        TabAction::Chat {
            tab: bad(),
            since: 0,
        },
        TabAction::Capture {
            action: TabCaptureAction::Start {
                tab: bad(),
                out: "x.cast".into(),
            },
        },
        TabAction::Capture {
            action: TabCaptureAction::Stop { tab: bad() },
        },
    ];
    for action in &actions {
        assert!(
            validate_user_tab_selectors(action).is_err(),
            "malformed mesh selector passed production transport preflight",
        );
    }
}

#[test]
fn remote_chat_path_percent_encodes_unicode_selector() {
    assert_eq!(
        remote_tab_chat_path("таб:one", 9),
        "/api/tabs/%D1%82%D0%B0%D0%B1%3Aone/chat?since=9",
    );
}

fn json_error_envelope(error: &crate::cmd::print::CliError) -> serde_json::Value {
    let mut out = Vec::new();
    let mut err = Vec::new();
    crate::cmd::print::write_cli_error(true, error, &mut out, &mut err);
    assert!(err.is_empty(), "--json puts the whole failure on stdout");
    serde_json::from_slice(&out).expect("stdout carries exactly one JSON envelope")
}

#[test]
fn invalid_remote_tab_selector_emits_the_envelope_with_no_rpc_code() {
    let message = validate_user_tab_selectors(&TabAction::Status {
        tab: Some("mesh:peer-without-a-tab".into()),
    })
    .expect_err("a mesh: selector without /<tab> is rejected before any RPC");

    let envelope = json_error_envelope(&crate::cmd::print::CliError::new(message));

    assert_eq!(envelope["ok"], serde_json::Value::Bool(false));
    assert_eq!(envelope["kind"], "error");
    assert_eq!(envelope["code"], serde_json::Value::Null);
    assert!(envelope["error"]
        .as_str()
        .expect("error is a string")
        .starts_with(INVALID_REMOTE_TAB_ID));
}

#[test]
fn mixed_local_and_remote_inspection_emits_the_envelope_with_no_rpc_code() {
    let message = remote_inspect_target(&["mesh:peer/tab_a".into(), "local_tab".into()])
        .expect_err("mixing local and remote inspection targets is rejected locally");

    let envelope = json_error_envelope(&crate::cmd::print::CliError::new(message));

    assert_eq!(envelope["ok"], serde_json::Value::Bool(false));
    assert_eq!(envelope["code"], serde_json::Value::Null);
    assert!(!envelope["error"]
        .as_str()
        .expect("error is a string")
        .contains("-32"));
}

struct StubRpc {
    method: Option<String>,
    params: Option<serde_json::Value>,
    failure: Option<crate::client::SocketCallError>,
}

impl StubRpc {
    fn failing(code: i32, message: &str) -> Self {
        Self {
            method: None,
            params: None,
            failure: Some(crate::client::SocketCallError::Rpc(
                crate::client::RpcError {
                    code,
                    message: message.into(),
                    data: None,
                },
            )),
        }
    }
}

impl crate::client::RpcClient for StubRpc {
    fn call_detailed(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, crate::client::SocketCallError> {
        self.method = Some(method.to_string());
        self.params = params;
        Err(self.failure.take().expect("stub answers exactly one call"))
    }
}

fn assert_converted(
    stub: &StubRpc,
    error: &crate::cmd::print::CliError,
    method: &str,
    params: &serde_json::Value,
) {
    assert_eq!(stub.method.as_deref(), Some(method));
    assert_eq!(stub.params.as_ref(), Some(params));
    assert_eq!(error.code, Some(-32603));
    assert!(!error.message.contains("-32"), "{}", error.message);
}

#[test]
fn focus_failure_moves_the_rpc_code_out_of_the_message() {
    let mut stub = StubRpc::failing(-32603, "Tab not found");

    let error = focus_tab(&mut stub, "zzzzzzzz").expect_err("the stub answers with a failure");

    assert_converted(&stub, &error, "tab.focus", &json!({ "tab_id": "zzzzzzzz" }));
}

#[test]
fn close_failure_moves_the_rpc_code_out_of_the_message() {
    let mut stub = StubRpc::failing(-32603, "Tab not found");
    let params = json!({ "tab_id": "zzzzzzzz", "caller_tab_id": "caller01" });

    let error = close_tab(&mut stub, params.clone()).expect_err("the stub answers with a failure");

    assert_converted(&stub, &error, "tab.close", &params);
}

#[test]
fn rename_failure_moves_the_rpc_code_out_of_the_message() {
    let mut stub = StubRpc::failing(-32603, "Tab not found");

    let error = rename_tab(&mut stub, "zzzzzzzz", "new title")
        .expect_err("the stub answers with a failure");

    assert_converted(
        &stub,
        &error,
        "tab.rename",
        &json!({ "tab_id": "zzzzzzzz", "title": "new title" }),
    );
}

#[test]
fn describe_failure_moves_the_rpc_code_out_of_the_message() {
    let mut stub = StubRpc::failing(-32603, "Tab not found");

    let error = describe_tab(&mut stub, "zzzzzzzz", "a summary")
        .expect_err("the stub answers with a failure");

    assert_converted(
        &stub,
        &error,
        "tab.describe",
        &json!({ "tab_id": "zzzzzzzz", "description": "a summary" }),
    );
}

#[test]
fn a_converted_tab_failure_renders_as_the_envelope_on_stdout() {
    let mut stub = StubRpc::failing(-32603, "Tab not found");
    let error = focus_tab(&mut stub, "zzzzzzzz").expect_err("the stub answers with a failure");

    let envelope = json_error_envelope(&error);

    assert_eq!(envelope["ok"], serde_json::Value::Bool(false));
    assert_eq!(envelope["kind"], "error");
    assert_eq!(envelope["code"], -32603);
    assert!(!envelope["error"]
        .as_str()
        .expect("error is a string")
        .contains("-32"));
}
