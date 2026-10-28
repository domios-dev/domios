pub fn directives(
    profile: &str,
    mut environment: impl FnMut(&str) -> Option<String>,
) -> Vec<String> {
    if profile == "debug" {
        return vec![
            "cargo:rustc-env=SERVER_DEV_FRONTEND_ORIGIN=".into(),
            "cargo:rustc-env=SERVER_DEV_IDENTIFIER=".into(),
        ];
    }
    let frontend = environment("CODETERM_SERVER_DEV_FRONTEND_ORIGIN").unwrap_or_default();
    assert!(
        frontend.bytes().all(|byte| byte.is_ascii_graphic()),
        "Invalid development frontend origin"
    );
    let identifier = environment("CODETERM_SERVER_DEV_IDENTIFIER").unwrap_or_default();
    assert!(identifier
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b".-".contains(&byte)));
    vec![
        "cargo:rerun-if-env-changed=CODETERM_SERVER_DEV_FRONTEND_ORIGIN".into(),
        format!("cargo:rustc-env=SERVER_DEV_FRONTEND_ORIGIN={frontend}"),
        "cargo:rerun-if-env-changed=CODETERM_SERVER_DEV_IDENTIFIER".into(),
        format!("cargo:rustc-env=SERVER_DEV_IDENTIFIER={identifier}"),
    ]
}

#[cfg(test)]
#[path = "build_frontend/tests.rs"]
mod tests;
