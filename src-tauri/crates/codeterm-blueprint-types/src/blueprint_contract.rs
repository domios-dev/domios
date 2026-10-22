use crate::blueprint_pipeline::BlueprintSpec;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FieldCheck {
    pub field: String,
    pub passed: bool,
    pub reason: Option<String>,
}
impl FieldCheck {
    pub fn new(field: impl Into<String>, result: Result<(), String>) -> Self {
        Self {
            field: field.into(),
            passed: result.is_ok(),
            reason: result.err(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PipelinePreview {
    pub normalized_spec: BlueprintSpec,
    pub chain: String,
    pub checks: Vec<FieldCheck>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expect_version: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApplyError {
    CreationOnly { field: String, message: String },
    Validation { checks: Vec<FieldCheck> },
    Refused { message: String },
}
impl From<String> for ApplyError {
    fn from(message: String) -> Self {
        Self::Refused { message }
    }
}
impl From<&str> for ApplyError {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}
impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreationOnly { field, message } => write!(f, "{field}: {message}"),
            Self::Refused { message } => f.write_str(message),
            Self::Validation { checks } => {
                for (index, check) in checks.iter().enumerate() {
                    if index > 0 {
                        f.write_str("; ")?;
                    }
                    write!(
                        f,
                        "{}: {}",
                        check.field,
                        check.reason.as_deref().unwrap_or("Validation failed")
                    )?;
                }
                Ok(())
            }
        }
    }
}
impl std::error::Error for ApplyError {}

#[cfg(test)]
mod tests;
