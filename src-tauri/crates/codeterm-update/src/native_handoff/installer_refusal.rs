use super::NativeUpdateHandoff;
use codeterm_types::native_update::apply::NativeUpdateApplyPhase;
use serde::Deserialize;
use std::io::Read;
use std::path::{Path, PathBuf};

pub fn path(root: &Path) -> PathBuf {
    root.join(".domios-installer-refusal.json")
}

#[derive(Deserialize)]
struct Refusal {
    target_version: String,
    message: String,
}

pub fn recover(root: &Path, record: &mut NativeUpdateHandoff) -> Result<bool, String> {
    if !matches!(
        record.result.phase,
        NativeUpdateApplyPhase::Applying | NativeUpdateApplyPhase::Relaunching
    ) {
        return Ok(false);
    }
    let mut bytes = Vec::new();
    match std::fs::File::open(path(root)) {
        Ok(file) => file
            .take(8193)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.to_string()),
    };
    if bytes.len() > 8192 {
        return Err("installer refusal exceeds size limit".into());
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    let refusal: Refusal = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if record.target_version.as_deref() != Some(refusal.target_version.as_str())
        || refusal.message.trim().is_empty()
    {
        return Ok(false);
    }
    record.advance(NativeUpdateApplyPhase::Failed {
        message: refusal.message,
    })?;
    Ok(true)
}
