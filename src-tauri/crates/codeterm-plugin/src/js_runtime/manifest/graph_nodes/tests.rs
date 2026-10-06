use crate::js_runtime::manifest::graph_nodes::{
    GraphNodesCap, NodePropertySchema, NodeScalarType, NodeSchema, MAX_SCHEMA_PROPERTIES,
};
use crate::js_runtime::manifest::PluginManifest;
use serde_json::json;

fn parse_cap(graph_nodes: &str) -> Result<GraphNodesCap, String> {
    let manifest = format!(
        r#"{{"id":"graph-fixture","version":"0.1.0","displayName":"Graph fixture",
             "capabilities":{{"graphNodes":{graph_nodes}}}}}"#
    );
    serde_json::from_str::<PluginManifest>(&manifest)
        .map(|m| m.capabilities.graph_nodes)
        .map_err(|e| e.to_string())
}

/// The params schema of a one-source declaration, so schema tests exercise the
/// real parse path.
fn schema(params: &str) -> NodeSchema {
    parse_cap(&format!(
        r#"{{"eventSources":[{{"kind":"fixture.event","title":"Fixture","params":{params}}}]}}"#
    ))
    .expect("schema parses")
    .event_sources
    .remove(0)
    .params
    .expect("params schema")
}

fn err_for(graph_nodes: &str) -> String {
    parse_cap(graph_nodes).expect_err("declaration must be refused")
}

// --- parse: both families ---

#[test]
fn graph_nodes_accepts_converters_and_event_sources() {
    let cap = parse_cap(
        r#"{
            "converters": [{"from":"audio","to":"text"}],
            "eventSources": [{
                "kind": "telegram.message",
                "title": "Telegram: new message",
                "params": {"type":"object","properties":{"chat_id":{"type":"string","maxLength":128}},"required":["chat_id"]},
                "payload": {"type":"object","properties":{"chat_id":{"type":"string"},"from":{"type":"string"},"text":{"type":"string","maxLength":16384}},"required":["chat_id","text"]}
            }]
        }"#,
    )
    .expect("graphNodes block parses");
    assert_eq!(cap.converters.len(), 1);
    assert_eq!(
        cap.converters[0],
        crate::converter::Conversion::new(
            codeterm_types::sentinel_input::MediaKind::Audio,
            codeterm_types::sentinel_input::MediaKind::Text
        )
    );
    assert_eq!(cap.event_sources.len(), 1);
    let source = &cap.event_sources[0];
    assert_eq!(source.kind, "telegram.message");
    assert_eq!(source.title, "Telegram: new message");
    let params = source.params.as_ref().expect("params schema");
    assert_eq!(params.properties.len(), 1);
    assert_eq!(params.required, vec!["chat_id".to_string()]);
    let payload = source.payload.as_ref().expect("payload schema");
    assert_eq!(payload.properties.len(), 3);
    assert!(payload
        .validate_instance(&json!({"chat_id":"1","text":"hi"}))
        .is_ok());
}

#[test]
fn graph_nodes_omitted_lists_default_to_empty_but_empty_block_is_refused() {
    let only_events =
        parse_cap(r#"{"eventSources":[{"kind":"a.b","title":"T"}]}"#).expect("events only");
    assert!(only_events.converters.is_empty());
    assert!(!only_events.is_empty());

    for empty in [
        "{}",
        r#"{"converters":[]}"#,
        r#"{"eventSources":[]}"#,
        r#"{"converters":[],"eventSources":[]}"#,
    ] {
        let err = err_for(empty);
        assert!(err.contains("capabilities.graphNodes"), "{err}");
        assert!(err.contains("at least one"), "{err}");
    }
}

#[test]
fn graph_nodes_is_object_only_no_bool_form() {
    for form in ["true", "false", r#""converters""#, "[]"] {
        let err = err_for(form);
        assert!(err.contains("capabilities.graphNodes"), "{err}");
    }
    let err = err_for(r#"{"converter":[{"from":"audio","to":"text"}]}"#);
    assert!(err.contains("unknown field `converter`"), "{err}");
}

#[test]
fn graph_nodes_round_trips_through_serialize() {
    let declared = r#"{"converters":[{"from":"video","to":"audio"}],"eventSources":[{"kind":"mail.inbox","title":"Mail","payload":{"type":"object","properties":{"subject":{"type":"string"}}}}]}"#;
    let cap = parse_cap(declared).expect("parse");
    let written = serde_json::to_value(&cap).expect("serialize");
    let again = serde_json::from_str::<GraphNodesCap>(&written.to_string()).expect("re-parse");
    assert_eq!(cap, again);
}

// --- converter family refusals ---

#[test]
fn graph_nodes_converters_refuse_bad_pairs() {
    let unknown = err_for(r#"{"converters":[{"from":"midi","to":"text"}]}"#);
    assert!(unknown.contains("converters[0].from"), "{unknown}");
    assert!(unknown.contains("unknown media kind \"midi\""), "{unknown}");

    let same = err_for(r#"{"converters":[{"from":"text","to":"text"}]}"#);
    assert!(same.contains("converters[0]"), "{same}");
    assert!(same.contains("both \"text\""), "{same}");

    let duplicate =
        err_for(r#"{"converters":[{"from":"audio","to":"text"},{"from":"audio","to":"text"}]}"#);
    assert!(duplicate.contains("converters[1]"), "{duplicate}");
    assert!(duplicate.contains("duplicate conversion"), "{duplicate}");

    let missing = err_for(r#"{"converters":[{"from":"audio"}]}"#);
    assert!(missing.contains("converters[0].to"), "{missing}");

    let stray = err_for(r#"{"converters":[{"from":"audio","to":"text","note":"x"}]}"#);
    assert!(stray.contains("unknown field `note`"), "{stray}");
}

#[test]
fn graph_nodes_converters_cap_at_sixteen() {
    let pair = r#"{"from":"audio","to":"text"}"#;
    let seventeen = (0..17).map(|_| pair).collect::<Vec<_>>().join(",");
    let err = err_for(&format!(r#"{{"converters":[{seventeen}]}}"#));
    assert!(err.contains("at most 16"), "{err}");

    // Every distinct non-self pair (4 kinds ⇒ 12 edges) parses under the cap.
    let kinds = ["text", "audio", "video", "image"];
    let mut pairs: Vec<String> = Vec::new();
    for from in kinds {
        for to in kinds {
            if from != to {
                pairs.push(format!(r#"{{"from":"{from}","to":"{to}"}}"#));
            }
        }
    }
    assert_eq!(pairs.len(), 12);
    let cap = parse_cap(&format!(r#"{{"converters":[{}]}}"#, pairs.join(",")))
        .expect("twelve distinct conversions parse");
    assert_eq!(cap.converters.len(), 12);
}

// --- eventSources family refusals ---

#[test]
fn graph_nodes_event_sources_refuse_bad_kind_title_and_duplicates() {
    for kind in [
        "telegram",
        "Telegram.message",
        "telegram.Message",
        "1telegram.message",
        ".message",
        "telegram.",
        "telegram..message",
        "telegram.mes sage",
        "telegram.mes-sage",
    ] {
        let err = err_for(&format!(
            r#"{{"eventSources":[{{"kind":"{kind}","title":"T"}}]}}"#
        ));
        assert!(err.contains("eventSources[0].kind"), "kind {kind}: {err}");
    }

    let long_kind = format!("a.{}", "b".repeat(64));
    let err = err_for(&format!(
        r#"{{"eventSources":[{{"kind":"{long_kind}","title":"T"}}]}}"#
    ));
    assert!(err.contains("at most 64 bytes"), "{err}");

    for title in [String::new(), "t".repeat(129)] {
        let err = err_for(&format!(
            r#"{{"eventSources":[{{"kind":"a.b","title":"{title}"}}]}}"#
        ));
        assert!(err.contains("eventSources[0].title"), "{err}");
    }

    let duplicate =
        err_for(r#"{"eventSources":[{"kind":"a.b","title":"One"},{"kind":"a.b","title":"Two"}]}"#);
    assert!(duplicate.contains("already declared"), "{duplicate}");

    let stray = err_for(r#"{"eventSources":[{"kind":"a.b","title":"T","chan":1}]}"#);
    assert!(stray.contains("unknown field `chan`"), "{stray}");

    let many = (0..=16)
        .map(|i| format!(r#"{{"kind":"src.event{i}","title":"T{i}"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let err = err_for(&format!(r#"{{"eventSources":[{many}]}}"#));
    assert!(err.contains("at most 16"), "{err}");
}

// --- NodeSchema subset ---

#[test]
fn graph_nodes_schema_accepts_scalar_and_array_properties() {
    let schema = schema(
        r#"{
            "type":"object",
            "properties":{
                "text":{"type":"string","description":"Body","maxLength":16384},
                "count":{"type":"integer"},
                "ratio":{"type":"number"},
                "seen":{"type":"boolean"},
                "tags":{"type":"array","items":{"type":"string","maxLength":32},"maxItems":8}
            },
            "required":["text"]
        }"#,
    );
    assert_eq!(schema.properties.len(), 5);
    assert_eq!(schema.required, vec!["text".to_string()]);
    let text = &schema
        .properties
        .iter()
        .find(|p| p.name == "text")
        .expect("text property");
    assert!(matches!(
        &text.schema,
        NodePropertySchema::Scalar(s) if s.kind == NodeScalarType::String && s.max_length == Some(16384)
    ));
    let tags = &schema
        .properties
        .iter()
        .find(|p| p.name == "tags")
        .expect("tags property");
    assert!(matches!(
        &tags.schema,
        NodePropertySchema::Array { max_items: 8, .. }
    ));
}

#[test]
fn graph_nodes_schema_refuses_the_open_json_schema_surface() {
    for (label, params) in [
        (
            "nested object",
            r#"{"type":"object","properties":{"a":{"type":"object"}}}"#,
        ),
        ("ref", r##"{"type":"object","$ref":"#/defs/x"}"##),
        ("oneOf", r#"{"type":"object","oneOf":[]}"#),
        (
            "anyOf",
            r#"{"type":"object","properties":{"a":{"anyOf":[]}}}"#,
        ),
        (
            "patternProperties",
            r#"{"type":"object","patternProperties":{}}"#,
        ),
        (
            "additionalProperties true",
            r#"{"type":"object","additionalProperties":true}"#,
        ),
        (
            "array of arrays",
            r#"{"type":"object","properties":{"a":{"type":"array","items":{"type":"array","maxItems":2},"maxItems":2}}}"#,
        ),
        (
            "object items",
            r#"{"type":"object","properties":{"a":{"type":"array","items":{"type":"object"},"maxItems":2}}}"#,
        ),
        (
            "root not object",
            r#"{"type":"array","items":{"type":"string"}}"#,
        ),
    ] {
        let err = err_for(&format!(
            r#"{{"eventSources":[{{"kind":"a.b","title":"T","params":{params}}}]}}"#
        ));
        assert!(err.contains("eventSources[0].params"), "{label}: {err}");
    }

    let ok = schema(
        r#"{"type":"object","properties":{"a":{"type":"string"}},"additionalProperties":false}"#,
    );
    assert_eq!(ok.properties.len(), 1);
}

#[test]
fn graph_nodes_schema_bounds_properties_keywords_and_required() {
    let too_many = (0..=MAX_SCHEMA_PROPERTIES)
        .map(|i| format!(r#""p{i}":{{"type":"string"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let err = err_for(&format!(
        r#"{{"eventSources":[{{"kind":"a.b","title":"T","params":{{"type":"object","properties":{{{too_many}}}}}}}]}}"#
    ));
    assert!(err.contains("at most 32 properties"), "{err}");

    for (label, params) in [
        (
            "required not declared",
            r#"{"type":"object","properties":{"a":{"type":"string"}},"required":["b"]}"#,
        ),
        (
            "maxLength on integer",
            r#"{"type":"object","properties":{"a":{"type":"integer","maxLength":8}}}"#,
        ),
        (
            "enum on integer",
            r#"{"type":"object","properties":{"a":{"type":"integer","enum":["1"]}}}"#,
        ),
        (
            "maxLength over cap",
            r#"{"type":"object","properties":{"a":{"type":"string","maxLength":65537}}}"#,
        ),
        (
            "maxItems over cap",
            r#"{"type":"object","properties":{"a":{"type":"array","items":{"type":"string"},"maxItems":257}}}"#,
        ),
        (
            "array without maxItems",
            r#"{"type":"object","properties":{"a":{"type":"array","items":{"type":"string"}}}}"#,
        ),
        (
            "array without items",
            r#"{"type":"object","properties":{"a":{"type":"array","maxItems":2}}}"#,
        ),
        (
            "unknown property keyword",
            r#"{"type":"object","properties":{"a":{"type":"string","pattern":"^x"}}}"#,
        ),
        (
            "enum not strings",
            r#"{"type":"object","properties":{"a":{"type":"string","enum":[1]}}}"#,
        ),
    ] {
        let err = err_for(&format!(
            r#"{{"eventSources":[{{"kind":"a.b","title":"T","params":{params}}}]}}"#
        ));
        assert!(err.contains("eventSources[0].params"), "{label}: {err}");
    }
}

// --- validate_instance table ---

#[test]
fn graph_nodes_validate_instance_table() {
    let schema = schema(
        r#"{
            "type":"object",
            "properties":{
                "text":{"type":"string","maxLength":8,"enum":["ok","fine"]},
                "count":{"type":"integer"},
                "flag":{"type":"boolean"},
                "tags":{"type":"array","items":{"type":"integer"},"maxItems":2}
            },
            "required":["text","count"]
        }"#,
    );

    for (label, instance, ok) in [
        (
            "full instance",
            json!({"text":"ok","count":3,"flag":true,"tags":[1,2]}),
            true,
        ),
        ("optional absent", json!({"text":"fine","count":0}), true),
        (
            "null not allowed for optional",
            json!({"text":"ok","count":1,"flag":null}),
            false,
        ),
        (
            "unknown property",
            json!({"text":"ok","count":1,"extra":"x"}),
            false,
        ),
        ("missing required", json!({"text":"ok"}), false),
        ("string type wrong", json!({"text":5,"count":1}), false),
        (
            "integer type wrong",
            json!({"text":"ok","count":1.5}),
            false,
        ),
        (
            "boolean type wrong",
            json!({"text":"ok","count":1,"flag":"yes"}),
            false,
        ),
        (
            "over-length string",
            json!({"text":"abcdefghij","count":1}),
            false,
        ),
        ("enum violation", json!({"text":"meh","count":1}), false),
        (
            "over-size array",
            json!({"text":"ok","count":1,"tags":[1,2,3]}),
            false,
        ),
        (
            "array item type wrong",
            json!({"text":"ok","count":1,"tags":["x"]}),
            false,
        ),
        ("instance not object", json!("text"), false),
    ] {
        let result = schema.validate_instance(&instance);
        assert_eq!(result.is_ok(), ok, "{label}: {result:?}");
    }
}

#[test]
fn graph_nodes_validate_instance_names_the_offending_path() {
    let schema = schema(
        r#"{"type":"object","properties":{"text":{"type":"string","maxLength":4}},"required":["text"]}"#,
    );
    let err = schema
        .validate_instance(&json!({"text":"toolong"}))
        .expect_err("over-length");
    assert_eq!(err.path, "$.text");
    assert!(err.to_string().contains("$.text"), "{err}");

    let err = schema
        .validate_instance(&json!({}))
        .expect_err("missing required");
    assert_eq!(err.path, "$.text");
    assert!(err.message.contains("missing required"), "{err}");

    let err = schema
        .validate_instance(&json!({"text":"ok","extra":1}))
        .expect_err("unknown prop");
    assert_eq!(err.path, "$.extra");
}
