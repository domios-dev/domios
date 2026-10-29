use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_SCHEMA_BYTES: usize = 65_536;
pub const MAX_SCHEMA_DEPTH: usize = 8;
pub const MAX_SCHEMA_NODES: usize = 256;
pub const MAX_VALUE_BYTES: usize = 65_536;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginSchema {
    wire: Value,
    node: SchemaNode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SchemaNode {
    nullable: bool,
    choices: Option<Vec<Value>>,
    kind: SchemaKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SchemaKind {
    Object {
        properties: BTreeMap<String, SchemaNode>,
        required: BTreeSet<String>,
    },
    Array {
        items: Box<SchemaNode>,
        minimum: usize,
        maximum: usize,
    },
    String {
        maximum: usize,
    },
    Number {
        integer: bool,
        minimum: Option<serde_json::Number>,
        maximum: Option<serde_json::Number>,
    },
    Boolean,
    Null,
    Variants {
        discriminator: String,
        variants: Vec<SchemaNode>,
    },
}

impl PluginSchema {
    pub fn from_value(wire: &Value) -> Result<Self, String> {
        if wire.to_string().len() > MAX_SCHEMA_BYTES {
            return Err("plugin schema exceeds 64 KiB".into());
        }
        let mut wire = wire.clone();
        let mut nodes = 0;
        let node = parse(&mut wire, 0, &mut nodes)?;
        Ok(Self { wire, node })
    }
    pub fn to_value(&self) -> Value {
        self.wire.clone()
    }
    pub fn validate_instance(&self, value: &Value) -> Result<(), String> {
        if value.to_string().len() > MAX_VALUE_BYTES {
            return Err("plugin value exceeds 64 KiB".into());
        }
        self.node.validate(value, "$")
    }
}

impl Serialize for PluginSchema {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.wire.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for PluginSchema {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::from_value(&Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

fn fields(object: &Map<String, Value>, allowed: &[&str]) -> Result<(), String> {
    for field in object.keys() {
        if !allowed.contains(&field.as_str()) {
            return Err(format!("unsupported plugin schema keyword {field}"));
        }
    }
    if object
        .get("description")
        .is_some_and(|v| v.as_str().is_none_or(|v| v.len() > 1024))
    {
        return Err("invalid schema description".into());
    }
    Ok(())
}

fn bound(
    object: &Map<String, Value>,
    name: &str,
    default: usize,
    maximum: usize,
) -> Result<usize, String> {
    match object.get(name) {
        None => Ok(default),
        Some(value) => value
            .as_u64()
            .filter(|v| *v <= maximum as u64)
            .map(|v| v as usize)
            .ok_or_else(|| format!("invalid schema {name}")),
    }
}

fn parse(wire: &mut Value, depth: usize, nodes: &mut usize) -> Result<SchemaNode, String> {
    *nodes += 1;
    if depth > MAX_SCHEMA_DEPTH || *nodes > MAX_SCHEMA_NODES {
        return Err("plugin schema exceeds depth/node bounds".into());
    }
    let object = wire
        .as_object_mut()
        .ok_or("plugin schema must be an object")?;
    if object.contains_key("oneOf") {
        return variants(object, depth, nodes);
    }
    let (kind, nullable) = match object.get("type") {
        Some(Value::String(kind)) => (kind.clone(), false),
        Some(Value::Array(kinds))
            if kinds.len() == 2
                && kinds.iter().filter(|v| v.as_str() == Some("null")).count() == 1 =>
        {
            (
                kinds
                    .iter()
                    .find_map(|v| v.as_str().filter(|v| *v != "null"))
                    .ok_or("invalid nullable type")?
                    .into(),
                true,
            )
        }
        _ => return Err("declare one schema type, optionally nullable".into()),
    };
    let choices = match (object.get("enum"), object.get("const")) {
        (Some(Value::Array(values)), None) if !values.is_empty() && values.len() <= 64 => {
            Some(values.clone())
        }
        (None, Some(value)) => Some(vec![value.clone()]),
        (None, None) => None,
        _ => return Err("declare at most one bounded enum or const".into()),
    };
    let scalar = ["type", "enum", "const", "description", "default"];
    let kind = match kind.as_str() {
        "object" => {
            fields(
                object,
                &[
                    "type",
                    "properties",
                    "required",
                    "additionalProperties",
                    "description",
                    "default",
                ],
            )?;
            if object
                .get("additionalProperties")
                .is_some_and(|v| v.as_bool() != Some(false))
            {
                return Err("plugin objects must be closed".into());
            }
            object.insert("additionalProperties".into(), Value::Bool(false));
            let mut properties = BTreeMap::new();
            if let Some(declared) = object.get_mut("properties") {
                let declared = declared
                    .as_object_mut()
                    .ok_or("properties must be an object")?;
                if declared.len() > 32 {
                    return Err("object exceeds 32 properties".into());
                }
                for (name, schema) in declared {
                    if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
                        return Err("invalid schema property name".into());
                    }
                    properties.insert(name.clone(), parse(schema, depth + 1, nodes)?);
                }
            }
            let mut required = BTreeSet::new();
            if let Some(declared) = object.get("required") {
                for name in declared.as_array().ok_or("required must be an array")? {
                    let name = name.as_str().ok_or("required must name a property")?;
                    if !properties.contains_key(name) || !required.insert(name.into()) {
                        return Err("invalid or duplicate required property".into());
                    }
                }
            }
            SchemaKind::Object {
                properties,
                required,
            }
        }
        "array" => {
            fields(
                object,
                &[
                    "type",
                    "items",
                    "minItems",
                    "maxItems",
                    "description",
                    "default",
                ],
            )?;
            if !object.contains_key("maxItems") {
                return Err("array requires maxItems".into());
            }
            let minimum = bound(object, "minItems", 0, 256)?;
            let maximum = bound(object, "maxItems", 0, 256)?;
            if minimum > maximum {
                return Err("array bounds are reversed".into());
            }
            let items = parse(
                object.get_mut("items").ok_or("array requires items")?,
                depth + 1,
                nodes,
            )?;
            SchemaKind::Array {
                items: Box::new(items),
                minimum,
                maximum,
            }
        }
        "string" => {
            fields(
                object,
                &[
                    "type",
                    "enum",
                    "const",
                    "maxLength",
                    "description",
                    "default",
                ],
            )?;
            SchemaKind::String {
                maximum: bound(object, "maxLength", MAX_VALUE_BYTES, MAX_VALUE_BYTES)?,
            }
        }
        "integer" | "number" => {
            fields(
                object,
                &[
                    "type",
                    "enum",
                    "const",
                    "minimum",
                    "maximum",
                    "description",
                    "default",
                ],
            )?;
            let number = |name| -> Result<Option<serde_json::Number>, String> {
                object
                    .get(name)
                    .map(|v| {
                        v.as_number()
                            .cloned()
                            .ok_or_else(|| format!("invalid {name}"))
                    })
                    .transpose()
            };
            let minimum = number("minimum")?;
            let maximum = number("maximum")?;
            if minimum
                .as_ref()
                .zip(maximum.as_ref())
                .is_some_and(|(a, b)| a.as_f64() > b.as_f64())
            {
                return Err("number bounds are reversed".into());
            }
            SchemaKind::Number {
                integer: kind == "integer",
                minimum,
                maximum,
            }
        }
        "boolean" => {
            fields(object, &scalar)?;
            SchemaKind::Boolean
        }
        "null" => {
            fields(object, &scalar)?;
            SchemaKind::Null
        }
        _ => return Err("unsupported plugin schema type".into()),
    };
    let node = SchemaNode {
        kind,
        nullable,
        choices: None,
    };
    if let Some(choices) = &choices {
        for value in choices {
            node.validate(value, "enum")?;
        }
    }
    let node = SchemaNode { choices, ..node };
    if let Some(value) = object.get("default") {
        node.validate(value, "default")?;
    }
    Ok(node)
}

fn variants(
    object: &mut Map<String, Value>,
    depth: usize,
    nodes: &mut usize,
) -> Result<SchemaNode, String> {
    fields(object, &["oneOf", "description"])?;
    let values = object
        .get_mut("oneOf")
        .and_then(Value::as_array_mut)
        .ok_or("oneOf must be an array")?;
    if values.len() < 2 || values.len() > 16 {
        return Err("oneOf needs 2..16 closed discriminated variants".into());
    }
    let variants = values
        .iter_mut()
        .map(|value| parse(value, depth + 1, nodes))
        .collect::<Result<Vec<_>, _>>()?;
    let Some(SchemaNode {
        kind: SchemaKind::Object {
            properties,
            required,
        },
        nullable: false,
        ..
    }) = variants.first()
    else {
        return Err("variants must be closed objects".into());
    };
    let discriminator = required
        .iter()
        .find(|name| {
            properties.get(*name).is_some_and(|p| {
                p.choices
                    .as_ref()
                    .is_some_and(|values| values.len() == 1 && values[0].is_string())
            }) && {
                let mut tags = BTreeSet::new();
                variants.iter().all(|variant| match &variant.kind {
                    SchemaKind::Object {
                        properties,
                        required,
                    } if !variant.nullable && required.contains(*name) => properties
                        .get(*name)
                        .and_then(|p| p.choices.as_ref())
                        .is_some_and(|values| {
                            values.len() == 1
                                && values[0]
                                    .as_str()
                                    .is_some_and(|tag| tags.insert(tag.to_owned()))
                        }),
                    _ => false,
                })
            }
        })
        .cloned()
        .ok_or("variants require one common required unique string const discriminator")?;
    Ok(SchemaNode {
        nullable: false,
        choices: None,
        kind: SchemaKind::Variants {
            discriminator,
            variants,
        },
    })
}

impl SchemaNode {
    fn validate(&self, value: &Value, path: &str) -> Result<(), String> {
        if self
            .choices
            .as_ref()
            .is_some_and(|choices| !choices.contains(value))
        {
            return Err(format!("{path}: outside enum/const"));
        }
        if self.nullable && value.is_null() {
            return Ok(());
        }
        let valid = match &self.kind {
            SchemaKind::Object {
                properties,
                required,
            } => value.as_object().is_some_and(|values| {
                required.iter().all(|name| values.contains_key(name))
                    && values.iter().all(|(name, value)| {
                        properties.get(name).is_some_and(|schema| {
                            schema.validate(value, &format!("{path}.{name}")).is_ok()
                        })
                    })
            }),
            SchemaKind::Array {
                items,
                minimum,
                maximum,
            } => value.as_array().is_some_and(|values| {
                values.len() >= *minimum
                    && values.len() <= *maximum
                    && values.iter().enumerate().all(|(index, value)| {
                        items.validate(value, &format!("{path}[{index}]")).is_ok()
                    })
            }),
            SchemaKind::String { maximum } => value
                .as_str()
                .is_some_and(|value| value.chars().count() <= *maximum),
            SchemaKind::Number {
                integer,
                minimum,
                maximum,
            } => value.as_f64().is_some_and(|value| {
                value.is_finite()
                    && (!*integer || value.fract() == 0.0 && value.abs() <= 9_007_199_254_740_991.0)
                    && minimum
                        .as_ref()
                        .and_then(serde_json::Number::as_f64)
                        .is_none_or(|min| value >= min)
                    && maximum
                        .as_ref()
                        .and_then(serde_json::Number::as_f64)
                        .is_none_or(|max| value <= max)
            }),
            SchemaKind::Boolean => value.is_boolean(),
            SchemaKind::Null => value.is_null(),
            SchemaKind::Variants {
                discriminator,
                variants,
            } => {
                value.get(discriminator).is_some()
                    && variants
                        .iter()
                        .filter(|schema| schema.validate(value, path).is_ok())
                        .count()
                        == 1
            }
        };
        valid
            .then_some(())
            .ok_or_else(|| format!("{path}: value violates closed plugin schema"))
    }
}

#[cfg(test)]
mod tests;
