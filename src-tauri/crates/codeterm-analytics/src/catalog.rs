use serde::{Deserialize, Serialize};

mod generated;
pub use generated::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Version {
    major: u16,
    minor: u16,
    patch: u16,
}

impl Version {
    pub fn from_release(value: &str) -> Option<Self> {
        let numeric = value.split(['-', '+']).next()?;
        serde_json::from_value(serde_json::Value::String(numeric.to_string())).ok()
    }

    pub fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl Serialize for Version {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!("{}.{}.{}", self.major, self.minor, self.patch))
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        let parts: Vec<_> = raw.split('.').collect();
        if parts.len() != 3
            || parts.iter().any(|part| {
                part.is_empty() || part.len() > 5 || !part.bytes().all(|c| c.is_ascii_digit())
            })
        {
            return Err(serde::de::Error::custom(
                "expected numeric major.minor.patch",
            ));
        }
        let parse = |part: &str| part.parse::<u16>().map_err(serde::de::Error::custom);
        Ok(Self::new(
            parse(parts[0])?,
            parse(parts[1])?,
            parse(parts[2])?,
        ))
    }
}
