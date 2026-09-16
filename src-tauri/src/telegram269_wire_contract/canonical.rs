use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::Deserializer as _;
use std::collections::HashSet;
use std::fmt;

#[derive(Debug)]
enum CanonicalJson {
    Null,
    Bool(bool),
    Integer(i128),
    Float(f64),
    String(String),
    Array(Vec<CanonicalJson>),
    Object(Vec<(String, CanonicalJson)>),
}

struct CanonicalJsonSeed;

impl<'de> DeserializeSeed<'de> for CanonicalJsonSeed {
    type Value = CanonicalJson;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(CanonicalJsonVisitor)
    }
}

struct CanonicalJsonVisitor;

impl<'de> Visitor<'de> for CanonicalJsonVisitor {
    type Value = CanonicalJson;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an RFC 8785 JSON value")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(CanonicalJson::Null)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(CanonicalJson::Null)
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(CanonicalJson::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(CanonicalJson::Integer(value.into()))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(CanonicalJson::Integer(value.into()))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if value.is_finite() {
            Ok(CanonicalJson::Float(value))
        } else {
            Err(E::custom("nonfinite_number"))
        }
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(CanonicalJson::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(CanonicalJson::String(value))
    }

    fn visit_seq<A>(self, mut values: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut result = Vec::new();
        while let Some(value) = values.next_element_seed(CanonicalJsonSeed)? {
            result.push(value);
        }
        Ok(CanonicalJson::Array(result))
    }

    fn visit_map<A>(self, mut values: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        while let Some(key) = values.next_key::<String>()? {
            if !seen.insert(key.clone()) {
                return Err(serde::de::Error::custom("duplicate_key"));
            }
            result.push((key, values.next_value_seed(CanonicalJsonSeed)?));
        }
        Ok(CanonicalJson::Object(result))
    }
}

fn write_json_string(out: &mut String, value: &str) {
    out.push('"');
    for scalar in value.chars() {
        match scalar {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{09}' => out.push_str("\\t"),
            '\u{0a}' => out.push_str("\\n"),
            '\u{0c}' => out.push_str("\\f"),
            '\u{0d}' => out.push_str("\\r"),
            scalar if scalar <= '\u{1f}' => out.push_str(&format!("\\u{:04x}", scalar as u32)),
            scalar => out.push(scalar),
        }
    }
    out.push('"');
}

fn write_canonical_json(out: &mut String, value: &CanonicalJson) {
    match value {
        CanonicalJson::Null => out.push_str("null"),
        CanonicalJson::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        CanonicalJson::Integer(value) => out.push_str(&value.to_string()),
        CanonicalJson::Float(value) if *value == 0.0 => out.push('0'),
        CanonicalJson::Float(value) if value.fract() == 0.0 => {
            out.push_str(&format!("{value:.0}"));
        }
        CanonicalJson::Float(value) => out.push_str(&value.to_string()),
        CanonicalJson::String(value) => write_json_string(out, value),
        CanonicalJson::Array(values) => {
            out.push('[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    out.push(',');
                }
                write_canonical_json(out, value);
            }
            out.push(']');
        }
        CanonicalJson::Object(values) => {
            let mut values: Vec<_> = values.iter().collect();
            values.sort_by(|left, right| left.0.encode_utf16().cmp(right.0.encode_utf16()));
            out.push('{');
            for (index, (key, value)) in values.into_iter().enumerate() {
                if index != 0 {
                    out.push(',');
                }
                write_json_string(out, key);
                out.push(':');
                write_canonical_json(out, value);
            }
            out.push('}');
        }
    }
}

pub(super) fn canonicalize_jcs(text: &str) -> Result<String, &'static str> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let value = CanonicalJsonSeed
        .deserialize(&mut deserializer)
        .map_err(|error| {
            if error.to_string().contains("duplicate_key") {
                "duplicate_key"
            } else {
                "malformed_json"
            }
        })?;
    deserializer.end().map_err(|_| "malformed_json")?;
    let mut result = String::new();
    write_canonical_json(&mut result, &value);
    Ok(result)
}

pub(super) fn require_canonical_jcs(text: &str) -> Result<(), &'static str> {
    if canonicalize_jcs(text)? == text {
        Ok(())
    } else {
        Err("noncanonical_json")
    }
}
