//! The ATP reference validator for `wasm-bindgen --target web`: each check returns the same
//! JSON report `atp-validate --json` prints.

use codeterm_team_package::{CheckOptions, HOST_PROFILES};
use wasm_bindgen::prelude::*;

/// Validates the one-file `team.yaml` form under host profile `none` or `domios`.
#[wasm_bindgen]
pub fn check_yaml(text: &str, profile: &str) -> Result<String, JsError> {
    let options = options(profile)?;
    Ok(codeterm_team_package::check_yaml(text, &options)
        .report
        .to_json())
}

/// Validates the `.team` ZIP form under host profile `none` or `domios`.
#[wasm_bindgen]
pub fn check_archive(bytes: &[u8], profile: &str) -> Result<String, JsError> {
    let options = options(profile)?;
    Ok(codeterm_team_package::check_archive(bytes, &options)
        .report
        .to_json())
}

fn options(profile: &str) -> Result<CheckOptions, JsError> {
    CheckOptions::for_profile(profile).ok_or_else(|| {
        JsError::new(&format!(
            "unknown host profile {profile}; expected one of {}",
            HOST_PROFILES.join(", ")
        ))
    })
}
