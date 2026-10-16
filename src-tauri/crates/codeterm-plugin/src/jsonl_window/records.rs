use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RecordSource {
    pub format: RecordFormat,
    pub filter: RecordFilter,
    #[serde(default)]
    pub max_record_bytes: Option<u64>,
    #[serde(default)]
    pub oversized: OversizedProjection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OversizedProjection {
    pub keep: Vec<Vec<String>>,
}

impl Default for OversizedProjection {
    fn default() -> Self {
        Self {
            keep: vec![vec!["type".into()]],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordFormat {
    Jsonl,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordFilter {
    #[serde(rename = "type")]
    pub record_type: Option<TypeFilter>,
    #[serde(default, rename = "where")]
    pub predicates: Vec<Predicate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypeFilter {
    pub path: Vec<String>,
    #[serde(rename = "in")]
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Predicate {
    pub path: Vec<String>,
    pub equals: Equality,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Equality {
    Placeholder { placeholder: Placeholder },
    Literal(Value),
}

impl<'de> Deserialize<'de> for Equality {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        if let Some(placeholder) = value.get("placeholder") {
            if value.as_object().is_none_or(|object| object.len() != 1)
                || placeholder != "sessionId"
            {
                return Err(serde::de::Error::custom(
                    "Only the sessionId record placeholder is supported",
                ));
            }
            return Ok(Self::Placeholder {
                placeholder: Placeholder::SessionId,
            });
        }
        Ok(Self::Literal(value))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Placeholder {
    SessionId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParsedRecord {
    pub offset: u64,
    pub row: usize,
    pub value: Value,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub oversized: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecordMeta {
    pub from_offset: u64,
    pub to_offset: u64,
    pub new_offset: u64,
    pub truncated_tail: bool,
    pub counts: RecordCounts,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordCounts {
    pub total: usize,
    pub matched: usize,
    pub invalid: usize,
    pub oversized: usize,
}

pub struct RecordWindow {
    pub records: Vec<ParsedRecord>,
    pub meta: RecordMeta,
    pub projection_error: Option<String>,
}

impl RecordSource {
    pub fn validate(&self) -> Result<(), String> {
        if self
            .max_record_bytes
            .is_some_and(|limit| limit == 0 || limit > super::MAX_RECORD_BYTES)
        {
            return Err("Record source limit exceeds the JSONL read boundary".into());
        }
        Ok(())
    }
}

fn field<'a>(record: &'a Value, path: &[String]) -> Option<&'a Value> {
    path.iter().try_fold(record, |value, key| value.get(key))
}

impl RecordFilter {
    pub fn matches(&self, record: &Value, session_id: Option<&str>) -> bool {
        self.record_type.as_ref().is_none_or(|filter| {
            field(record, &filter.path)
                .and_then(Value::as_str)
                .is_some_and(|kind| filter.values.iter().any(|allowed| allowed == kind))
        }) && self.predicates.iter().all(|predicate| {
            let value = field(record, &predicate.path);
            match &predicate.equals {
                Equality::Literal(expected) => value == Some(expected),
                Equality::Placeholder {
                    placeholder: Placeholder::SessionId,
                } => {
                    session_id.is_some_and(|session| value.and_then(Value::as_str) == Some(session))
                }
            }
        })
    }
}

pub fn parse_records(
    bytes: &[u8],
    from: u64,
    source: &RecordSource,
    session: Option<&str>,
) -> RecordWindow {
    parse_records_with_limit(bytes, from, source, session, super::MAX_RECORD_BYTES)
}

pub fn parse_records_with_limit(
    bytes: &[u8],
    from: u64,
    source: &RecordSource,
    session: Option<&str>,
    limit: u64,
) -> RecordWindow {
    let mut window = RecordWindow {
        records: Vec::new(),
        projection_error: None,
        meta: RecordMeta {
            from_offset: from,
            to_offset: from + bytes.len() as u64,
            new_offset: from,
            ..Default::default()
        },
    };
    for (row, line) in bytes.split_inclusive(|byte| *byte == b'\n').enumerate() {
        if line.last() != Some(&b'\n') {
            window.meta.truncated_tail = true;
            break;
        }
        let offset = window.meta.new_offset;
        window.meta.new_offset += line.len() as u64;
        window.meta.counts.total += 1;
        let oversized = line.len() as u64 > source.max_record_bytes.unwrap_or(limit).min(limit);
        window.meta.counts.oversized += usize::from(oversized);
        let text = String::from_utf8_lossy(line);
        if oversized && serde_json::from_str::<&serde_json::value::RawValue>(&text).is_err() {
            window.meta.counts.invalid += 1;
            continue;
        }
        let parsed = if oversized {
            let mut paths = source.oversized.keep.clone();
            paths.push(vec!["type".into()]);
            if let Some(filter) = &source.filter.record_type {
                paths.push(filter.path.clone());
            }
            paths.extend(
                source
                    .filter
                    .predicates
                    .iter()
                    .map(|predicate| predicate.path.clone()),
            );
            super::projection::project(&text, &paths, limit as usize)
        } else {
            serde_json::from_str(&text).map_err(|error| error.to_string())
        };
        let value: Value = match parsed {
            Ok(value) => value,
            Err(error) => {
                if oversized {
                    window.projection_error = Some(error);
                    break;
                }
                window.meta.counts.invalid += 1;
                continue;
            }
        };
        if !source.filter.matches(&value, session) {
            continue;
        }
        let value = if oversized {
            super::projection::retain_paths(&value, &source.oversized.keep)
        } else {
            value
        };
        window.records.push(ParsedRecord {
            offset,
            row,
            value,
            oversized,
            bytes: oversized.then_some(line.len() as u64),
        });
        window.meta.counts.matched += 1;
    }
    window
}

pub fn read_records(
    path: &Path,
    from: u64,
    budget: u64,
    source: &RecordSource,
    session: Option<&str>,
) -> std::io::Result<RecordWindow> {
    let cap = budget.min(super::DELTA_CHUNK_BYTES);
    let chunk = super::read_records_from(path, from, cap)?;
    let mut window = parse_records(&chunk.bytes, from, source, session);
    let total = std::fs::metadata(path)?.len();
    window.meta.to_offset = from.saturating_add(cap).min(total).max(from);
    window.meta.truncated_tail = chunk.new_offset < window.meta.to_offset;
    if chunk.bytes.is_empty() && chunk.new_offset > from {
        window.meta.counts.total = 1;
        window.meta.counts.oversized = 1;
    }
    window.meta.new_offset = chunk.new_offset;
    Ok(window)
}

#[cfg(test)]
mod tests;
