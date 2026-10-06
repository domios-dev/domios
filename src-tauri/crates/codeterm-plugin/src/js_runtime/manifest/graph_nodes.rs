//! `graphNodes` — a plugin's graph node declarations: media conversions the
//! host chains by declared capability, and typed, bounded event sources.
//!
//! Parsing is strict and closed: no bool form, no raw schema `Value`s, and none
//! of the open JSON Schema surface (`$ref`, combinators, nested objects).
//! Errors are field-named so a manifest typo names the exact key.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::converter::Conversion;
use codeterm_types::sentinel_input::MediaKind;

pub const MAX_CONVERTERS: usize = 16;
pub const MAX_EVENT_SOURCES: usize = 16;
pub const MAX_SCHEMA_PROPERTIES: usize = 32;
pub const MAX_STRING_LENGTH: u32 = 65536;
pub const MAX_ARRAY_ITEMS: u32 = 256;

const ROOT: &str = "capabilities.graphNodes";

/// `capabilities.graphNodes` — converters and event sources, both optional
/// lists; a present block must declare at least one node.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GraphNodesCap {
    pub converters: Vec<Conversion>,
    pub event_sources: Vec<EventSourceDecl>,
}

impl GraphNodesCap {
    pub fn is_empty(&self) -> bool {
        self.converters.is_empty() && self.event_sources.is_empty()
    }

    pub fn enabled(&self) -> bool {
        !self.is_empty()
    }
}

/// One declared event source: a stable `kind`, a user-facing `title`, and the
/// typed schemas its params and event payload must satisfy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventSourceDecl {
    pub kind: String,
    pub title: String,
    pub params: Option<NodeSchema>,
    pub payload: Option<NodeSchema>,
}

/// A closed, bounded schema: an object of scalar or one-level scalar-array
/// properties.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSchema {
    pub properties: Vec<NodeProperty>,
    pub required: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeProperty {
    pub name: String,
    pub schema: NodePropertySchema,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodePropertySchema {
    Scalar(NodeScalarSchema),
    Array {
        description: Option<String>,
        items: Box<NodeScalarSchema>,
        max_items: u32,
    },
}

/// A scalar type with its bounds: `enum` and `maxLength` for strings only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeScalarSchema {
    pub description: Option<String>,
    pub kind: NodeScalarType,
    pub enum_values: Option<Vec<String>>,
    pub max_length: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeScalarType {
    String,
    Integer,
    Number,
    Boolean,
}

/// A failed instance check, pointing at the offending path (`$.text`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaError {
    pub path: String,
    pub message: String,
}

impl SchemaError {
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

impl std::error::Error for SchemaError {}

impl NodeSchema {
    /// Pure instance check: unknown properties, missing `required`, wrong
    /// types, over-length strings, values outside an `enum`, over-size arrays.
    pub fn validate_instance(&self, instance: &Value) -> Result<(), SchemaError> {
        let Some(object) = instance.as_object() else {
            return Err(SchemaError::new("$", "expected an object"));
        };
        for (name, value) in object {
            let Some(property) = self.properties.iter().find(|p| p.name == name.as_str()) else {
                return Err(SchemaError::new(format!("$.{name}"), "unknown property"));
            };
            property.schema.validate(value, &format!("$.{name}"))?;
        }
        for name in &self.required {
            if !object.contains_key(name) {
                return Err(SchemaError::new(
                    format!("$.{name}"),
                    "missing required property",
                ));
            }
        }
        Ok(())
    }
}

impl NodePropertySchema {
    fn validate(&self, value: &Value, path: &str) -> Result<(), SchemaError> {
        match self {
            Self::Scalar(scalar) => scalar.validate(value, path),
            Self::Array {
                items, max_items, ..
            } => {
                let Some(array) = value.as_array() else {
                    return Err(SchemaError::new(path, "expected an array"));
                };
                if array.len() as u32 > *max_items {
                    return Err(SchemaError::new(
                        path,
                        format!("array length {} exceeds maxItems {max_items}", array.len()),
                    ));
                }
                for (index, item) in array.iter().enumerate() {
                    items.validate(item, &format!("{path}[{index}]"))?;
                }
                Ok(())
            }
        }
    }
}

impl NodeScalarSchema {
    fn validate(&self, value: &Value, path: &str) -> Result<(), SchemaError> {
        match self.kind {
            NodeScalarType::String => {
                let Some(text) = value.as_str() else {
                    return Err(SchemaError::new(path, "expected a string"));
                };
                if let Some(max) = self.max_length {
                    let length = text.chars().count() as u32;
                    if length > max {
                        return Err(SchemaError::new(
                            path,
                            format!("string length {length} exceeds maxLength {max}"),
                        ));
                    }
                }
                if let Some(values) = &self.enum_values {
                    if !values.iter().any(|allowed| allowed == text) {
                        return Err(SchemaError::new(path, "value outside the declared enum"));
                    }
                }
                Ok(())
            }
            NodeScalarType::Integer => {
                if value.is_i64() || value.is_u64() {
                    Ok(())
                } else {
                    Err(SchemaError::new(path, "expected an integer"))
                }
            }
            NodeScalarType::Number => {
                if value.is_number() {
                    Ok(())
                } else {
                    Err(SchemaError::new(path, "expected a number"))
                }
            }
            NodeScalarType::Boolean => {
                if value.is_boolean() {
                    Ok(())
                } else {
                    Err(SchemaError::new(path, "expected a boolean"))
                }
            }
        }
    }
}

// --- wire shape: strict parse with field-named errors, symmetric serialization ---

impl Serialize for GraphNodesCap {
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_value().serialize(s)
    }
}

impl<'de> Deserialize<'de> for GraphNodesCap {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(d)?;
        Self::from_value(&value).map_err(serde::de::Error::custom)
    }
}

impl GraphNodesCap {
    pub fn to_value(&self) -> Value {
        let mut object = Map::new();
        if !self.converters.is_empty() {
            object.insert(
                "converters".into(),
                Value::Array(
                    self.converters
                        .iter()
                        .map(|conversion| {
                            serde_json::json!({
                                "from": conversion.from.as_str(),
                                "to": conversion.to.as_str(),
                            })
                        })
                        .collect(),
                ),
            );
        }
        if !self.event_sources.is_empty() {
            object.insert(
                "eventSources".into(),
                Value::Array(
                    self.event_sources
                        .iter()
                        .map(EventSourceDecl::to_value)
                        .collect(),
                ),
            );
        }
        Value::Object(object)
    }

    pub fn from_value(value: &Value) -> Result<Self, String> {
        let object = value
            .as_object()
            .ok_or_else(|| format!("{ROOT}: expected an object"))?;
        for key in object.keys() {
            if key != "converters" && key != "eventSources" {
                return Err(format!("{ROOT}: unknown field `{key}`"));
            }
        }
        let converters = match object.get("converters") {
            None => Vec::new(),
            Some(declared) => parse_converters(declared)?,
        };
        let event_sources = match object.get("eventSources") {
            None => Vec::new(),
            Some(declared) => parse_event_sources(declared)?,
        };
        if converters.is_empty() && event_sources.is_empty() {
            return Err(format!(
                "{ROOT}: declare at least one converter or event source"
            ));
        }
        Ok(Self {
            converters,
            event_sources,
        })
    }
}

fn parse_converters(value: &Value) -> Result<Vec<Conversion>, String> {
    let path = format!("{ROOT}.converters");
    let list = value
        .as_array()
        .ok_or_else(|| format!("{path}: expected an array"))?;
    if list.len() > MAX_CONVERTERS {
        return Err(format!(
            "{path}: at most {MAX_CONVERTERS} conversions are allowed"
        ));
    }
    let mut declared: Vec<Conversion> = Vec::with_capacity(list.len());
    for (index, entry) in list.iter().enumerate() {
        let entry_path = format!("{path}[{index}]");
        let object = entry
            .as_object()
            .ok_or_else(|| format!("{entry_path}: expected an object"))?;
        for key in object.keys() {
            if key != "from" && key != "to" {
                return Err(format!("{entry_path}: unknown field `{key}`"));
            }
        }
        let from = parse_media_kind(object.get("from"), &format!("{entry_path}.from"))?;
        let to = parse_media_kind(object.get("to"), &format!("{entry_path}.to"))?;
        if from == to {
            return Err(format!(
                "{entry_path}: from and to are both \"{}\"",
                from.as_str()
            ));
        }
        if let Some(previous) = declared.iter().position(|c| c.from == from && c.to == to) {
            return Err(format!(
                "{entry_path}: duplicate conversion {} -> {} (already declared at index {previous})",
                from.as_str(),
                to.as_str()
            ));
        }
        declared.push(Conversion::new(from, to));
    }
    Ok(declared)
}

fn parse_media_kind(value: Option<&Value>, path: &str) -> Result<MediaKind, String> {
    let Some(raw) = value.and_then(Value::as_str) else {
        return Err(format!("{path}: expected a media kind"));
    };
    MediaKind::parse(raw).ok_or_else(|| format!("{path}: unknown media kind \"{raw}\""))
}

fn parse_event_sources(value: &Value) -> Result<Vec<EventSourceDecl>, String> {
    let path = format!("{ROOT}.eventSources");
    let list = value
        .as_array()
        .ok_or_else(|| format!("{path}: expected an array"))?;
    if list.len() > MAX_EVENT_SOURCES {
        return Err(format!(
            "{path}: at most {MAX_EVENT_SOURCES} event sources are allowed"
        ));
    }
    let mut declared: Vec<EventSourceDecl> = Vec::with_capacity(list.len());
    for (index, entry) in list.iter().enumerate() {
        let entry_path = format!("{path}[{index}]");
        let object = entry
            .as_object()
            .ok_or_else(|| format!("{entry_path}: expected an object"))?;
        for key in object.keys() {
            if !matches!(key.as_str(), "kind" | "title" | "params" | "payload") {
                return Err(format!("{entry_path}: unknown field `{key}`"));
            }
        }
        let kind = parse_kind(object.get("kind"), &format!("{entry_path}.kind"))?;
        if declared.iter().any(|source| source.kind == kind) {
            return Err(format!(
                "{entry_path}.kind: \"{kind}\" is already declared by this plugin"
            ));
        }
        let title = parse_title(object.get("title"), &format!("{entry_path}.title"))?;
        let params = match object.get("params") {
            None | Some(Value::Null) => None,
            Some(schema) => Some(parse_node_schema(schema, &format!("{entry_path}.params"))?),
        };
        let payload = match object.get("payload") {
            None | Some(Value::Null) => None,
            Some(schema) => Some(parse_node_schema(schema, &format!("{entry_path}.payload"))?),
        };
        declared.push(EventSourceDecl {
            kind,
            title,
            params,
            payload,
        });
    }
    Ok(declared)
}

/// `kind` is a namespaced machine id: at least two lowercase dot-separated
/// parts (`telegram.message`), at most 64 bytes.
fn parse_kind(value: Option<&Value>, path: &str) -> Result<String, String> {
    let Some(raw) = value.and_then(Value::as_str) else {
        return Err(format!("{path}: expected a namespaced kind string"));
    };
    if raw.len() > 64 {
        return Err(format!("{path}: must be at most 64 bytes"));
    }
    let parts: Vec<&str> = raw.split('.').collect();
    let valid_part = |part: &str| {
        let mut chars = part.chars();
        matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    };
    if parts.len() < 2 || !parts.iter().all(|part| valid_part(part)) {
        return Err(format!(
            "{path}: \"{raw}\" must match ^[a-z][a-z0-9_]*(\\.[a-z][a-z0-9_]*)+$"
        ));
    }
    Ok(raw.to_string())
}

fn parse_title(value: Option<&Value>, path: &str) -> Result<String, String> {
    let Some(raw) = value.and_then(Value::as_str) else {
        return Err(format!("{path}: expected a title string"));
    };
    if raw.is_empty() || raw.len() > 128 {
        return Err(format!("{path}: must be 1-128 bytes"));
    }
    Ok(raw.to_string())
}

fn parse_node_schema(value: &Value, path: &str) -> Result<NodeSchema, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{path}: expected an object schema"))?;
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "type" | "properties" | "required" | "additionalProperties"
        ) {
            return Err(format!(
                "{path}: unsupported schema field `{key}` (no $ref, combinators or patternProperties)"
            ));
        }
    }
    if object.get("type").and_then(Value::as_str) != Some("object") {
        return Err(format!("{path}.type: expected \"object\""));
    }
    if let Some(additional) = object.get("additionalProperties") {
        if additional.as_bool() != Some(false) {
            return Err(format!(
                "{path}.additionalProperties: only `false` is supported"
            ));
        }
    }
    let mut properties: Vec<NodeProperty> = Vec::new();
    if let Some(declared) = object.get("properties") {
        let map = declared
            .as_object()
            .ok_or_else(|| format!("{path}.properties: expected an object"))?;
        if map.len() > MAX_SCHEMA_PROPERTIES {
            return Err(format!(
                "{path}.properties: at most {MAX_SCHEMA_PROPERTIES} properties are allowed"
            ));
        }
        for (name, schema) in map {
            properties.push(NodeProperty {
                name: name.clone(),
                schema: parse_property_schema(schema, &format!("{path}.properties.{name}"))?,
            });
        }
    }
    let mut required: Vec<String> = Vec::new();
    if let Some(declared) = object.get("required") {
        let list = declared
            .as_array()
            .ok_or_else(|| format!("{path}.required: expected an array"))?;
        for (index, entry) in list.iter().enumerate() {
            let name = entry
                .as_str()
                .ok_or_else(|| format!("{path}.required[{index}]: expected a property name"))?;
            if !properties.iter().any(|p| p.name == name) {
                return Err(format!(
                    "{path}.required[{index}]: \"{name}\" is not a declared property"
                ));
            }
            required.push(name.to_string());
        }
    }
    Ok(NodeSchema {
        properties,
        required,
    })
}

fn parse_property_schema(value: &Value, path: &str) -> Result<NodePropertySchema, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{path}: expected an object schema"))?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{path}.type: expected a scalar or array type"))?;
    let description = parse_description(object.get("description"), &format!("{path}.description"))?;
    match kind {
        "array" => {
            for key in object.keys() {
                if !matches!(key.as_str(), "type" | "description" | "items" | "maxItems") {
                    return Err(format!("{path}: unsupported schema field `{key}`"));
                }
            }
            let items = object
                .get("items")
                .ok_or_else(|| format!("{path}.items: required for array properties"))?;
            let items = parse_scalar_schema(items, &format!("{path}.items"))?;
            let max_items = object
                .get("maxItems")
                .and_then(Value::as_u64)
                .filter(|n| *n <= MAX_ARRAY_ITEMS as u64)
                .ok_or_else(|| {
                    format!("{path}.maxItems: expected an integer <= {MAX_ARRAY_ITEMS}")
                })? as u32;
            Ok(NodePropertySchema::Array {
                description,
                items: Box::new(items),
                max_items,
            })
        }
        "object" => Err(format!("{path}.type: nested objects are not supported")),
        _ => {
            for key in object.keys() {
                if !matches!(key.as_str(), "type" | "description" | "enum" | "maxLength") {
                    return Err(format!("{path}: unsupported schema field `{key}`"));
                }
            }
            Ok(NodePropertySchema::Scalar(parse_scalar_fields(
                object,
                path,
                description,
            )?))
        }
    }
}

fn parse_scalar_schema(value: &Value, path: &str) -> Result<NodeScalarSchema, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{path}: expected a scalar schema"))?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{path}.type: expected a scalar type"))?;
    if matches!(kind, "array" | "object") {
        return Err(format!(
            "{path}.type: array items must be scalars (one level of scalar arrays)"
        ));
    }
    for key in object.keys() {
        if !matches!(key.as_str(), "type" | "description" | "enum" | "maxLength") {
            return Err(format!("{path}: unsupported schema field `{key}`"));
        }
    }
    let description = parse_description(object.get("description"), &format!("{path}.description"))?;
    parse_scalar_fields(object, path, description)
}

fn parse_scalar_fields(
    object: &Map<String, Value>,
    path: &str,
    description: Option<String>,
) -> Result<NodeScalarSchema, String> {
    let raw_kind = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let kind = match raw_kind {
        "string" => NodeScalarType::String,
        "integer" => NodeScalarType::Integer,
        "number" => NodeScalarType::Number,
        "boolean" => NodeScalarType::Boolean,
        other => {
            return Err(format!(
                "{path}.type: unknown type \"{other}\" (expected string, integer, number, boolean or array)"
            ))
        }
    };
    let enum_values = match object.get("enum") {
        None | Some(Value::Null) => None,
        Some(declared) => {
            if kind != NodeScalarType::String {
                return Err(format!(
                    "{path}.enum: only string properties may declare enum"
                ));
            }
            let list = declared
                .as_array()
                .ok_or_else(|| format!("{path}.enum: expected an array of strings"))?;
            if list.is_empty() {
                return Err(format!("{path}.enum: expected at least one value"));
            }
            let mut values: Vec<String> = Vec::with_capacity(list.len());
            for (index, entry) in list.iter().enumerate() {
                values.push(
                    entry
                        .as_str()
                        .map(str::to_string)
                        .ok_or_else(|| format!("{path}.enum[{index}]: expected a string"))?,
                );
            }
            Some(values)
        }
    };
    let max_length = match object.get("maxLength") {
        None | Some(Value::Null) => None,
        Some(declared) => {
            if kind != NodeScalarType::String {
                return Err(format!(
                    "{path}.maxLength: only string properties may declare maxLength"
                ));
            }
            let length = declared
                .as_u64()
                .filter(|n| *n <= MAX_STRING_LENGTH as u64)
                .ok_or_else(|| {
                    format!("{path}.maxLength: expected an integer <= {MAX_STRING_LENGTH}")
                })? as u32;
            Some(length)
        }
    };
    Ok(NodeScalarSchema {
        description,
        kind,
        enum_values,
        max_length,
    })
}

fn parse_description(value: Option<&Value>, path: &str) -> Result<Option<String>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(declared) => Ok(Some(
            declared
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{path}: expected a string"))?,
        )),
    }
}

impl EventSourceDecl {
    pub fn to_value(&self) -> Value {
        let mut object = Map::new();
        object.insert("kind".into(), Value::String(self.kind.clone()));
        object.insert("title".into(), Value::String(self.title.clone()));
        if let Some(params) = &self.params {
            object.insert("params".into(), params.to_value());
        }
        if let Some(payload) = &self.payload {
            object.insert("payload".into(), payload.to_value());
        }
        Value::Object(object)
    }
}

impl NodeSchema {
    pub fn to_value(&self) -> Value {
        let mut properties = Map::new();
        for property in &self.properties {
            properties.insert(property.name.clone(), property.schema.to_value());
        }
        let mut object = Map::new();
        object.insert("type".into(), Value::String("object".into()));
        object.insert("properties".into(), Value::Object(properties));
        if !self.required.is_empty() {
            object.insert(
                "required".into(),
                Value::Array(
                    self.required
                        .iter()
                        .map(|name| Value::String(name.clone()))
                        .collect(),
                ),
            );
        }
        Value::Object(object)
    }
}

impl NodePropertySchema {
    fn to_value(&self) -> Value {
        match self {
            Self::Scalar(scalar) => scalar.to_value(),
            Self::Array {
                description,
                items,
                max_items,
            } => {
                let mut object = Map::new();
                object.insert("type".into(), Value::String("array".into()));
                if let Some(description) = description {
                    object.insert("description".into(), Value::String(description.clone()));
                }
                object.insert("items".into(), items.to_value());
                object.insert("maxItems".into(), Value::from(*max_items));
                Value::Object(object)
            }
        }
    }
}

impl NodeScalarSchema {
    fn to_value(&self) -> Value {
        let mut object = Map::new();
        object.insert(
            "type".into(),
            Value::String(
                match self.kind {
                    NodeScalarType::String => "string",
                    NodeScalarType::Integer => "integer",
                    NodeScalarType::Number => "number",
                    NodeScalarType::Boolean => "boolean",
                }
                .into(),
            ),
        );
        if let Some(description) = &self.description {
            object.insert("description".into(), Value::String(description.clone()));
        }
        if let Some(values) = &self.enum_values {
            object.insert(
                "enum".into(),
                Value::Array(values.iter().map(|v| Value::String(v.clone())).collect()),
            );
        }
        if let Some(max) = self.max_length {
            object.insert("maxLength".into(), Value::from(max));
        }
        Value::Object(object)
    }
}

#[cfg(test)]
mod tests;
