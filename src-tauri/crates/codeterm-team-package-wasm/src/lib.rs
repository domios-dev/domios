//! The ATP reference validator for `wasm-bindgen --target web`: each check returns the same
//! JSON report `atp-validate --json` prints. Validation is format-only.

use codeterm_team_package::CheckOptions;
use wasm_bindgen::prelude::*;

/// Validates the one-file `team.yaml` form.
#[wasm_bindgen]
pub fn check_yaml(text: &str) -> String {
    codeterm_team_package::check_yaml(text, &CheckOptions::default())
        .report
        .to_json()
}

/// Validates the `.team` ZIP form.
#[wasm_bindgen]
pub fn check_archive(bytes: &[u8]) -> String {
    codeterm_team_package::check_archive(bytes, &CheckOptions::default())
        .report
        .to_json()
}
