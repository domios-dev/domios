use serde::ser::{self, Serialize, SerializeMap, SerializeSeq, SerializeStruct};
use serde_json::{Map, Value};

use super::{filler, private_key, IDS};

#[derive(Clone, Copy, Default)]
pub(super) struct Sanitizer {
    preserve_string: bool,
    typed_map_values: bool,
}

type Error = serde_json::Error;
type Result<T> = std::result::Result<T, Error>;

fn error(message: impl std::fmt::Display) -> Error {
    ser::Error::custom(message)
}

macro_rules! scalar {
    ($($method:ident($ty:ty)),* $(,)?) => {$(
        fn $method(self, value: $ty) -> Result<Value> {
            serde::Serializer::$method(serde_json::value::Serializer, value)
        }
    )*};
}

impl serde::Serializer for Sanitizer {
    type Ok = Value;
    type Error = Error;
    type SerializeSeq = Sequence;
    type SerializeTuple = Sequence;
    type SerializeTupleStruct = Sequence;
    type SerializeTupleVariant = ser::Impossible<Value, Error>;
    type SerializeMap = Object;
    type SerializeStruct = Object;
    type SerializeStructVariant = ser::Impossible<Value, Error>;

    scalar! {
        serialize_bool(bool), serialize_i8(i8), serialize_i16(i16), serialize_i32(i32),
        serialize_i64(i64), serialize_i128(i128), serialize_u8(u8), serialize_u16(u16),
        serialize_u32(u32), serialize_u64(u64), serialize_u128(u128),
        serialize_f32(f32), serialize_f64(f64),
    }

    fn serialize_str(self, value: &str) -> Result<Value> {
        Ok(Value::String(if self.preserve_string {
            value.into()
        } else {
            filler(value)
        }))
    }

    fn serialize_char(self, value: char) -> Result<Value> {
        self.serialize_str(&value.to_string())
    }

    fn serialize_bytes(self, _: &[u8]) -> Result<Value> {
        Err(error("Unsupported plan byte payload"))
    }

    fn serialize_none(self) -> Result<Value> {
        Ok(Value::Null)
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Value> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Value> {
        Ok(Value::Null)
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result<Value> {
        Ok(Value::Null)
    }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<Value> {
        Ok(Value::String(variant.into()))
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<Value> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<Value> {
        Err(error("Unsupported plan newtype variant"))
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Sequence> {
        Ok(Sequence {
            sanitizer: self,
            values: Vec::with_capacity(len.unwrap_or(0)),
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<Sequence> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(self, _: &'static str, len: usize) -> Result<Sequence> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        Err(error("Unsupported plan tuple variant"))
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Object> {
        let mut object = Object::new("");
        object.typed_map_values = self.typed_map_values;
        Ok(object)
    }

    fn serialize_struct(self, name: &'static str, _: usize) -> Result<Object> {
        Ok(Object::new(name))
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant> {
        Err(error("Unsupported plan struct variant"))
    }
}

pub(super) struct Sequence {
    sanitizer: Sanitizer,
    values: Vec<Value>,
}

impl SerializeSeq for Sequence {
    type Ok = Value;
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        self.values.push(value.serialize(self.sanitizer)?);
        Ok(())
    }

    fn end(self) -> Result<Value> {
        Ok(Value::Array(self.values))
    }
}

macro_rules! sequence {
    ($trait:ident, $method:ident) => {
        impl ser::$trait for Sequence {
            type Ok = Value;
            type Error = Error;
            fn $method<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
                SerializeSeq::serialize_element(self, value)
            }
            fn end(self) -> Result<Value> {
                SerializeSeq::end(self)
            }
        }
    };
}
sequence!(SerializeTuple, serialize_element);
sequence!(SerializeTupleStruct, serialize_field);

pub(super) struct Object {
    name: &'static str,
    values: Map<String, Value>,
    key: Option<String>,
    typed_map_values: bool,
}

impl Object {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            values: Map::new(),
            key: None,
            typed_map_values: false,
        }
    }
}

impl SerializeStruct for Object {
    type Ok = Value;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<()> {
        // Serde emits internally tagged enum discriminants as ordinary struct strings.
        let tag = matches!(
            (self.name, key),
            ("PlannerRef" | "PlanningAuthor", "kind") | ("PlanPauseStepOutcome", "state")
        );
        let role = matches!(
            (self.name, key),
            ("TaskExecutionAttempt", "role") | ("Task", "taken_by_role")
        );
        let preserve_role = role
            && serde_json::to_value(value)?.as_str().is_some_and(|role| {
                codeterm_types::agent::roles::AgentRole::from_str_compat(role).is_some()
            });
        let preserve_report_status = self.name == "TaskExecutionReportSnapshot"
            && key == "status"
            && serde_json::from_value::<codeterm_types::agent::ReportStatus>(serde_json::to_value(
                value,
            )?)
            .is_ok();
        let preserve_continuation = self.name == "AttemptContinuationState"
            && key == "state"
            && serde_json::to_value(value)?.as_str().is_some_and(|state| {
                matches!(
                    state,
                    "awaiting_continuation"
                        | "executor_gone"
                        | "continuation_delivering"
                        | "continued"
                        | "bounce_continuation_failed"
                )
            });
        let sanitizer = Sanitizer {
            preserve_string: tag
                || preserve_role
                || preserve_report_status
                || preserve_continuation
                || IDS.contains(&key),
            typed_map_values: self.name == "ControlLoop" && key == "agent_states",
        };
        self.values.insert(key.into(), value.serialize(sanitizer)?);
        Ok(())
    }

    fn end(self) -> Result<Value> {
        Ok(Value::Object(self.values))
    }
}

impl SerializeMap for Object {
    type Ok = Value;
    type Error = Error;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<()> {
        let raw = serde_json::to_value(key)?;
        let key = raw
            .as_str()
            .ok_or_else(|| error("Plan map key must be a string"))?;
        self.key = Some(private_key(key, &self.values).map_err(error)?);
        Ok(())
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        let key = self
            .key
            .take()
            .ok_or_else(|| error("Plan map value missing key"))?;
        let safe = if self.typed_map_values {
            value.serialize(Sanitizer::default())?
        } else {
            let raw = serde_json::to_value(value)?;
            super::walk(&raw, "", true, super::SchemaLocation::None).map_err(error)?
        };
        self.values.insert(key, safe);
        Ok(())
    }

    fn end(self) -> Result<Value> {
        Ok(Value::Object(self.values))
    }
}
