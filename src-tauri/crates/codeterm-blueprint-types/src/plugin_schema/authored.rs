use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum SchemaTypeName {
    Object,
    Array,
    String,
    Integer,
    Number,
    Boolean,
    Null,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(untagged)]
pub enum SchemaType {
    Single(SchemaTypeName),
    Nullable(Vec<SchemaTypeName>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthoredSchema {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub kind: Option<SchemaType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub properties: Option<BTreeMap<String, AuthoredSchema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub required: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub additional_properties: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub items: Option<Box<AuthoredSchema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub min_items: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub max_items: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub max_length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub minimum: Option<serde_json::Number>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub maximum: Option<serde_json::Number>,
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub enum_values: Option<Vec<Value>>,
    #[serde(
        rename = "const",
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub constant: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub one_of: Option<Vec<AuthoredSchema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub default: Option<Value>,
}

fn present<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}
