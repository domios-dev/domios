//! The plugin's agent guide: a markdown file inside its bundle, named by the
//! manifest's `agentGuide`, read for the mem skill item and `plugin get`.

use std::io::Read;
use std::path::{Component, Path};

pub const MAX_AGENT_GUIDE_BYTES: usize = 16 * 1024;
pub const AGENT_GUIDE_TRUNCATED_MARKER: &str = "\n\n[agent guide truncated]";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentGuide {
    pub text: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentGuideError {
    /// Absolute, parent-relative, drive-qualified or otherwise non-plain path.
    InvalidPath,
    Missing,
    /// Resolves (e.g. through a symlink) outside the bundle directory.
    OutsideBundle,
    NotUtf8,
    Empty,
    Read(String),
}

impl std::fmt::Display for AgentGuideError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPath => {
                f.write_str("agentGuide must be a relative path inside the plugin")
            }
            Self::Missing => f.write_str("agentGuide file does not exist"),
            Self::OutsideBundle => f.write_str("agentGuide resolves outside the plugin directory"),
            Self::NotUtf8 => f.write_str("agentGuide is not valid UTF-8"),
            Self::Empty => f.write_str("agentGuide is empty"),
            Self::Read(error) => write!(f, "agentGuide could not be read: {error}"),
        }
    }
}

/// Reads at most [`MAX_AGENT_GUIDE_BYTES`]; a longer guide is cut at a char
/// boundary and ends with [`AGENT_GUIDE_TRUNCATED_MARKER`].
pub fn load_agent_guide(
    bundle_dir: &Path,
    relative_path: &str,
) -> Result<AgentGuide, AgentGuideError> {
    if !valid_relative_path(relative_path) {
        return Err(AgentGuideError::InvalidPath);
    }
    let candidate = bundle_dir.join(relative_path);
    if !candidate.exists() {
        return Err(AgentGuideError::Missing);
    }
    let root =
        std::fs::canonicalize(bundle_dir).map_err(|e| AgentGuideError::Read(e.to_string()))?;
    let resolved =
        std::fs::canonicalize(&candidate).map_err(|e| AgentGuideError::Read(e.to_string()))?;
    if !resolved.starts_with(&root) {
        return Err(AgentGuideError::OutsideBundle);
    }
    if !resolved.is_file() {
        return Err(AgentGuideError::Missing);
    }
    let file = std::fs::File::open(&resolved).map_err(|e| AgentGuideError::Read(e.to_string()))?;
    let mut bytes = Vec::with_capacity(MAX_AGENT_GUIDE_BYTES + 1);
    file.take((MAX_AGENT_GUIDE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| AgentGuideError::Read(e.to_string()))?;
    bound_agent_guide(bytes)
}

fn bound_agent_guide(mut bytes: Vec<u8>) -> Result<AgentGuide, AgentGuideError> {
    let truncated = bytes.len() > MAX_AGENT_GUIDE_BYTES;
    bytes.truncate(MAX_AGENT_GUIDE_BYTES);
    let mut text = match String::from_utf8(bytes) {
        Ok(text) => text,
        // A cut can split the last char; only that incomplete tail is dropped.
        Err(error) if truncated && error.utf8_error().error_len().is_none() => {
            let valid = error.utf8_error().valid_up_to();
            let mut bytes = error.into_bytes();
            bytes.truncate(valid);
            String::from_utf8(bytes).map_err(|_| AgentGuideError::NotUtf8)?
        }
        Err(_) => return Err(AgentGuideError::NotUtf8),
    };
    let trimmed_len = text.trim_end().len();
    text.truncate(trimmed_len);
    if text.trim().is_empty() {
        return Err(AgentGuideError::Empty);
    }
    if truncated {
        text.push_str(AGENT_GUIDE_TRUNCATED_MARKER);
    }
    Ok(AgentGuide { text, truncated })
}

fn valid_relative_path(path: &str) -> bool {
    !path.is_empty()
        && path == path.trim()
        && !path.contains('\0')
        && !path.contains('\\')
        && !path.contains(':')
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

#[cfg(test)]
mod tests;
