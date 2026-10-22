use super::*;
use crate::blueprint_pipeline::{BlueprintResult, BlueprintSpec, Step, StepKind, Trigger};
use serde::de::{DeserializeOwned, Error, Visitor};
use std::collections::BTreeSet;

type DeError = serde::de::value::Error;
struct Fields;
impl<'de> serde::Deserializer<'de> for Fields {
    type Error = DeError;
    fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, DeError> {
        Err(DeError::custom("expected struct"))
    }
    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        fields: &'static [&'static str],
        _: V,
    ) -> Result<V::Value, DeError> {
        Err(DeError::custom(serde_json::to_string(fields).unwrap()))
    }
    serde::forward_to_deserialize_any! { bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string bytes byte_buf option unit unit_struct newtype_struct seq tuple tuple_struct map enum identifier ignored_any }
}
fn fields<T: DeserializeOwned>(variant: Option<&'static str>) -> BTreeSet<String> {
    if let Some(variant) = variant {
        let tag = if std::any::type_name::<T>() == std::any::type_name::<Trigger>() {
            "type"
        } else {
            "kind"
        };
        let mut value = serde_json::Map::new();
        value.insert(tag.into(), json!(variant));
        value.insert("__schema_parity_probe".into(), Value::Null);
        let error = serde_json::from_value::<T>(Value::Object(value))
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.starts_with("unknown field `__schema_parity_probe`"),
            "{error}"
        );
        let mut fields: BTreeSet<String> = error
            .split('`')
            .skip(3)
            .step_by(2)
            .map(str::to_owned)
            .collect();
        fields.insert(tag.into());
        return fields;
    }
    let error = T::deserialize(Fields)
        .err()
        .expect("field introspection stops deserialization");
    serde_json::from_str(&error.to_string()).unwrap()
}
fn properties(schema: &Value) -> BTreeSet<String> {
    schema["properties"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect()
}

#[test]
fn blueprint_schema_pipeline_fields_match_serde_types() {
    let schema = schema();
    let pipeline = &schema["properties"]["pipeline"];
    let mut spec = fields::<BlueprintSpec>(None);
    for common in ["name", "note", "owner"] {
        assert!(spec.remove(common));
        assert!(schema["properties"].get(common).is_some());
    }
    assert_eq!(properties(pipeline), spec);
    assert_eq!(
        properties(&pipeline["properties"]["result"]),
        fields::<BlueprintResult>(None)
    );
    for trigger in pipeline["properties"]["trigger"]["oneOf"]
        .as_array()
        .unwrap()
    {
        let variant = match trigger["properties"]["type"]["const"].as_str().unwrap() {
            "schedule" => "schedule",
            "input" => "input",
            "event" => "event",
            _ => panic!("unknown trigger"),
        };
        assert_eq!(properties(trigger), fields::<Trigger>(Some(variant)));
    }
    let variants = pipeline["properties"]["steps"]["items"]["oneOf"]
        .as_array()
        .unwrap();
    assert_eq!(variants.len(), StepKind::DESCRIPTORS.len());
    for descriptor in StepKind::DESCRIPTORS {
        let name = descriptor.name;
        let variant = variants
            .iter()
            .find(|s| {
                if name == "command" {
                    s["properties"].get("run").is_some()
                } else {
                    s["properties"]["uses"]["const"] == name
                }
            })
            .unwrap();
        let mut step = fields::<Step>(None);
        assert!(step.remove("kind"));
        if name == "command" {
            step.insert("run".into());
        } else {
            step.extend(["uses".into(), "with".into()]);
        }
        assert_eq!(properties(variant), step, "{name} step fields");
        let mut params = fields::<StepKind>(Some(name));
        params.remove("kind");
        match name {
            "command" => assert_eq!(params, BTreeSet::from(["command".into()])),
            "http" | "github" => {
                assert_eq!(params, BTreeSet::from(["probe".into()]));
                let probe = if name == "http" {
                    fields::<crate::blueprint_http::HttpProbe>(None)
                } else {
                    fields::<crate::blueprint_github::GithubProbe>(None)
                };
                assert_eq!(properties(&variant["properties"]["with"]), probe);
            }
            _ => assert_eq!(
                properties(&variant["properties"]["with"]),
                params,
                "{name} parameters"
            ),
        }
    }
}
