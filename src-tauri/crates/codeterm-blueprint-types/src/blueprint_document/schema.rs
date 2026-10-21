use serde_json::{json, Value};

pub fn schema() -> Value {
    let scalar = json!({"type":"string"});
    let io = json!({"enum":["text","file","audio","video","image"]});
    let mut step_variants = Vec::new();
    step_variants.push(json!({"type":"object","required":["name","input","run"],"additionalProperties":false,"properties":{
        "name":scalar,"input":io,"run":{"type":"string","minLength":1,"maxLength":16384},"on":{"$ref":"#/$defs/routes"}}}));
    for kind in crate::blueprint_catalog::STEPS
        .iter()
        .filter(|node| node.name != "command")
    {
        let (required, properties) = match kind.name {
            "http" => (
                vec!["url"],
                json!({"url":scalar,"timeout_ms":{"type":"integer","minimum":1,"maximum":10000},"expected_status":{"anyOf":[scalar,{"type":"integer","minimum":100,"maximum":599}]}}),
            ),
            "github" => (
                vec!["resource"],
                json!({"repo":scalar,"org":scalar,"resource":{"enum":["run","jobs","pr_checks","runners"]},"id":{"anyOf":[{"type":"integer","minimum":1},scalar]},"label":scalar,"timeout_ms":{"type":"integer","minimum":1,"maximum":60000}}),
            ),
            "ask" => (
                vec!["question", "buckets"],
                json!({"question":scalar,"buckets":{"type":"array","items":scalar}}),
            ),
            "filter" => (
                vec!["pattern"],
                json!({"pattern":scalar,"bucket":{"anyOf":[scalar,{"type":"null"}]}}),
            ),
            "convert" => (vec!["from", "to"], json!({"from":io,"to":io})),
            "agent" => (
                vec![
                    "provider",
                    "profile",
                    "task",
                    "deadline_ms",
                    "max_tokens",
                    "max_turns",
                ],
                json!({"provider":scalar,"profile":scalar,"task":scalar,"deadline_ms":{"type":"integer","minimum":1,"maximum":3600000},"max_tokens":{"type":"integer","minimum":1,"maximum":1000000},"max_turns":{"type":"integer","minimum":1,"maximum":100}}),
            ),
            _ => continue,
        };
        step_variants.push(json!({"type":"object","required":["name","input","uses","with"],"additionalProperties":false,"properties":{
            "name":scalar,"input":io,"uses":{"const":kind.name},"with":{"type":"object","required":required,"additionalProperties":false,"properties":properties},"on":{"$ref":"#/$defs/routes"}}}));
    }
    let handlers = json!({"type":"object","additionalProperties":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["type"],"properties":{"id":scalar,"type":{"enum":["notify","stop","send","cron","blueprint","command","respond_prompt","escalate"]},"parameters":{"type":"object"}}}}});
    let watch = json!({"type":"object","additionalProperties":false,"required":["question","buckets"],"properties":{
        "probe":scalar,"observe":{"anyOf":[scalar,{"type":"object"}]},"input":{"type":"object"},"terminal":{"type":"object"},"session":{"type":"boolean"},
        "question":{"type":"string","minLength":1,"maxLength":4096},"buckets":{"type":"array","minItems":1,"maxItems":16,"items":scalar},
        "every":scalar,"routes":{"$ref":"#/$defs/routes"},"handlers":handlers,
        "matches":{"type":"object","additionalProperties":scalar},"model_classification":{"type":"boolean"},"rearm_on_change":{"type":"array","items":scalar},
        "until":{"anyOf":[scalar,{"type":"null"}]},"timeout":scalar,"paused":{"type":"boolean"},"escalation_timeout_ms":{"type":"integer","minimum":1},
        "max_transitions":{"type":"integer","minimum":1,"maximum":256},"max_actions":{"type":"integer","minimum":1,"maximum":1024}
    }});
    let pipeline = json!({"type":"object","additionalProperties":false,"required":["trigger","steps"],"properties":{
        "trigger":{"oneOf":[
            {"type":"object","required":["type","every"],"additionalProperties":false,"properties":{"type":{"const":"schedule"},"every":scalar}},
            {"type":"object","required":["type","kind","source"],"additionalProperties":false,"properties":{"type":{"const":"input"},"kind":io,"source":{"$ref":"#/$defs/data"}}},
            {"type":"object","required":["type","kind","params"],"additionalProperties":false,"properties":{"type":{"const":"event"},"kind":scalar,"params":{"type":"object"}}}]},
        "steps":{"type":"array","minItems":1,"maxItems":64,"items":{"oneOf":step_variants}},
        "result":{"type":"object","additionalProperties":false,"properties":{"to":{"type":"string","pattern":"^(owner|team|notify|tab:.+)$"}}}
    }});
    json!({
        "$schema":"https://json-schema.org/draft/2020-12/schema",
        "title":"Domios Blueprint","type":"object","additionalProperties":false,
        "required":["schema_version","name"],
        "oneOf":[{"required":["watch"],"not":{"required":["pipeline"]}},{"required":["pipeline"],"not":{"required":["watch"]}}],
        "allOf":[{"if":{"required":["watch"]},"then":{"not":{"required":["owner"]}}}],
        "properties":{
            "schema_version":{"const":1},"name":{"type":"string","minLength":3,"maxLength":40},
            "note":{"anyOf":[{"type":"string","maxLength":200},{"type":"null"}]},"owner":{"enum":["team","tab"],"default":"team"},
            "inputs":{"type":"object","additionalProperties":{"type":"object","additionalProperties":false,"required":["type"],"properties":{
                "type":{"enum":["string","integer","number","boolean","object","array"]},"default":{},"description":scalar,"enum":{"type":"array","minItems":1}}}},
            "watch":{"$ref":"#/$defs/watch"},
            "pipeline":pipeline
        },
        "$defs":{
            "routes":{"type":"object","additionalProperties":{"type":"string","minLength":1}},
            "data":{"type":"object","oneOf":[{"required":["text"]},{"required":["file"]},{"required":["url"]}],"additionalProperties":false,"properties":{"text":scalar,"file":scalar,"url":scalar}},
            "watch":watch
        }
    })
}
