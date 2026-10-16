use indexmap::IndexMap;
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum OrderedValue {
    Scalar(Value),
    Array(Vec<Self>),
    Object(IndexMap<String, Self>),
}

impl OrderedValue {
    pub(crate) fn to_value(&self) -> Value {
        match self {
            Self::Scalar(value) => value.clone(),
            Self::Array(values) => Value::Array(values.iter().map(Self::to_value).collect()),
            Self::Object(values) => Value::Object(
                values
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_value()))
                    .collect(),
            ),
        }
    }

    pub(crate) fn retain_shape(&self, shape: &Value) -> Self {
        match (self, shape) {
            (Self::Object(values), Value::Object(shape)) => Self::Object(
                values
                    .iter()
                    .filter_map(|(key, value)| {
                        shape
                            .get(key)
                            .map(|shape| (key.clone(), value.retain_shape(shape)))
                    })
                    .collect(),
            ),
            (Self::Array(values), Value::Array(shape)) => Self::Array(
                values
                    .iter()
                    .zip(shape)
                    .map(|(value, shape)| value.retain_shape(shape))
                    .collect(),
            ),
            _ => Self::Scalar(shape.clone()),
        }
    }
}

impl Serialize for OrderedValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Scalar(value) => value.serialize(serializer),
            Self::Array(values) => values.serialize(serializer),
            Self::Object(values) => values.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for OrderedValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(OrderedVisitor)
    }
}

struct OrderedVisitor;

impl<'de> Visitor<'de> for OrderedVisitor {
    type Value = OrderedValue;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        Ok(OrderedValue::Scalar(Value::Null))
    }

    fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Self::Value, E> {
        Ok(OrderedValue::Scalar(value.into()))
    }

    fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(OrderedValue::Scalar(value.into()))
    }

    fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
        Ok(OrderedValue::Scalar(value.into()))
    }

    fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
        Ok(OrderedValue::Scalar(value.into()))
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
        self.visit_string(value.to_owned())
    }

    fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
        Ok(OrderedValue::Scalar(value.into()))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = access.next_element()? {
            values.push(value);
        }
        Ok(OrderedValue::Array(values))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let mut values = IndexMap::new();
        while let Some((key, value)) = access.next_entry()? {
            values.insert(key, value);
        }
        Ok(OrderedValue::Object(values))
    }
}
