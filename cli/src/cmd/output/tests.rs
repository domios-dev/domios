use super::*;
use serde_json::json;

#[test]
fn compact_output_exact_lines_for_search_and_list_fixtures() {
    let row = json!({"id":"one","title":"First\nresult","body":"line one\nline two", "score":0.87,"relevance":0.95});
    for key in ["hits", "plans", "tasks", "tabs", "blueprints", "skills"] {
        let value = json!({(key): [row.clone()], "total": 3, "warnings":["unavailable"]});
        assert_eq!(compact_list(&value, None), "one  First result — line one line two\n2 more — refine the query or use --limit\n", "{key}");
    }
    assert_eq!(compact_list(&json!([row]), Some("one")), "one  First result (self) — line one line two\n");
}

#[test]
fn compact_output_search_sources_and_unicode_snippets() {
    let value = json!({"hits":[
        {"item":{"id":"memory","title":"Memory","summary":"a\nb"},"score":0.5},
        {"source":"session","session_id":"session","title":"Session","excerpt":"prior run"},
        {"source":"doc","doc":"CLI","section":"Commands","pointer":"domios docs CLI"},
        {"meta":{"session_id":"history","title":"History"},"snippets":[{"text":"matched text"}]},
        {"id":"long","title":"Unicode","body":"界".repeat(81)}
    ]});
    assert_eq!(compact_list(&value, None), format!(
        "memory  Memory — a b\nsession  Session — prior run\ndomios docs CLI  Commands — domios docs CLI\nhistory  History — matched text\nlong  Unicode — {}…\n", "界".repeat(80)));
    assert_eq!(compact_list(&json!({"hits":[], "warnings":["offline"]}), None), "");
}

#[test]
fn compact_output_named_handlers_use_the_shared_renderer() {
    let rows = json!({"hits":[{"item":{"id":"m","title":"Title","body":"Body"}}]});
    assert_eq!(crate::cmd::mem::render_search_hits(&rows), "m  Title — Body\n");
    assert_eq!(crate::cmd::mem::render_search_hits(&json!({"hits":[]})), "");
}

#[test]
fn json_shape_contract_for_each_collection_payload() {
    for key in COLLECTION_KEYS {
        let input = json!({(*key):[{"id":"one","title":"First"}], "total":2, "next":"cursor", "warnings":["offline"]});
        assert_eq!(envelope(&input, true), json!({
            "result":[{"id":"one","title":"First"}],
            "meta":{"total":2}, "next":"cursor", "warnings":["offline"]
        }), "{key}");
    }
}

#[test]
fn json_shape_contract_preserves_objects_scalars_and_nested_collections() {
    for input in [json!({"task_id":"task_one", "tasks":[]}), json!("text"), json!(null), json!(3), json!([])] {
        assert_eq!(envelope(&input, false)["result"], crate::cmd::print::uniform_id_value(&input));
    }
    let input = json!({"tabs":[],"scope":{"kind":"workspace"},"workspaces":[]});
    let output = envelope(&input, true);
    assert_eq!(output["result"], json!([]));
    assert_eq!(output["meta"]["scope"], input["scope"]);
    assert_eq!(output["meta"]["workspaces"], input["workspaces"]);
}

#[test]
fn json_shape_contract_checklists_keep_metadata_outside_result_rows() {
    let value = json!({"checklist":{"generation":2,"items":[{"id":"todo","title":"Task","state":"open"}]}});
    let output = envelope(&value, true);
    assert_eq!(output["result"], value["checklist"]["items"]);
    assert_eq!(output["meta"]["checklist"], json!({"generation":2}));
    assert_eq!(compact_list(&value, None), "todo  Task — open\n");
}

#[test]
fn json_shape_contract_error_receipts_use_result() {
    let value = crate::cmd::error_contract::error_envelope("refused", None, Some(&json!({"code":"refused","send_id":"own","bytes_written":false})));
    let output = envelope(&value, false);
    assert_eq!(output["result"]["send_id"], "own");
    assert_eq!(output["ok"], false);
    assert!(output.get("data").is_none());
    assert_eq!(output["error"]["code"], "refused");
}

#[test]
fn json_shape_guard_rejects_old_envelope_keys() {
    for value in [json!([]), json!({"tasks":[]}), json!({"hits":[]}), json!({"results":[]}), json!({"data":[]}), json!({"id":"one"})] {
        let output = envelope(&value, true);
        let object = output.as_object().unwrap();
        assert!(object.contains_key("result"));
        for key in ["data", "results", "hits", "tasks", "tabs", "plans"] {
            assert!(!object.contains_key(key), "{key}: {output}");
        }
    }
}

#[test]
fn verbose_flag_is_available_on_all_named_list_commands() {
    use clap::Parser;
    for args in [
        vec!["domios", "mem", "search", "fixture", "--verbose"],
        vec!["domios", "sessions", "search", "fixture", "--verbose"],
        vec!["domios", "plan", "list", "--verbose"],
        vec!["domios", "plan", "task", "list", "pl_fixture", "--verbose"],
        vec!["domios", "tab", "list", "--verbose"],
        vec!["domios", "blueprint", "list", "--verbose"],
    ] {
        assert!(crate::Cli::try_parse_from(&args).unwrap().verbose, "{args:?}");
    }
}
