use super::*;

#[test]
fn explicit_path_modules_resolve_children_beside_the_source_file() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/module_paths/root.rs");
    let mut rows = Vec::new();
    walk(&root, root.parent().unwrap(), "", false, &mut rows, None);
    assert_eq!(
        rows,
        vec![
            serde_json::json!({
                "name": "relocated::tests::beside_explicit_source",
                "platform": true,
                "ignore_reason": "fixture-only native operation",
            }),
            serde_json::json!({
                "name": "conventional::tests::inside_module_directory",
                "platform": false,
                "ignore_reason": null,
            }),
        ]
    );
}

fn classify(source: &str) -> Vec<serde_json::Value> {
    let file = syn::parse_file(source).unwrap();
    let mut output = Vec::new();
    items(
        &file.items,
        Path::new("."),
        Path::new("."),
        "",
        false,
        &mut output,
        None,
    );
    output
}

#[test]
fn resolves_generated_include_from_exact_cargo_output() {
    let tokens = quote::quote!(concat!(env!("OUT_DIR"), "/generated.rs"));
    assert_eq!(
        include_path(tokens, Path::new("src"), Some(Path::new("/build/out"))),
        PathBuf::from("/build/out/generated.rs")
    );
    assert_eq!(
        include_path(quote::quote!("tests.rs"), Path::new("src"), None),
        PathBuf::from("src/tests.rs")
    );
}

#[test]
#[should_panic(expected = "Cargo build-script OUT_DIR")]
fn missing_generated_directory_refuses_classification() {
    include_path(
        quote::quote!(concat!(env!("OUT_DIR"), "/generated.rs")),
        Path::new("src"),
        None,
    );
}

#[test]
fn inherits_inline_module_cfg() {
    let rows = classify("#[cfg(unix)] mod tests { #[test] fn native() {} } #[test] fn shared() {}");
    assert_eq!(rows[0]["name"], "tests::native");
    assert_eq!(rows[0]["platform"], true);
    assert_eq!(rows[1]["platform"], false);
}

#[test]
fn detects_platform_cfg_inside_function() {
    let rows = classify("#[tokio::test] async fn native() { #[cfg(windows)] { run(); } }");
    assert_eq!(rows[0]["platform"], true);
}

#[test]
fn fixture_gate_removal_moves_test_to_shared() {
    let before = classify("#[cfg_attr(not(windows), ignore)] #[test] fn fixture() {}");
    let after = classify("#[test] fn fixture() {}");
    assert_eq!(before[0]["platform"], true);
    assert_eq!(after[0]["platform"], false);
}

#[test]
fn classifies_cfg_macro_and_generated_test_scopes() {
    let rows = classify("#[test] fn native() { if cfg!(windows) {} } macro_rules! cases { () => { #[test] fn generated() {} }; } cases!();");
    assert_eq!(rows[0]["platform"], true);
    assert_eq!(rows[1]["scope"], true);
    assert_eq!(rows[1]["platform"], false);
}

#[test]
fn preserves_explicit_ignore_reasons_only() {
    let rows = classify("#[ignore = \"TOKEN_REASON\"] #[test] fn named() {} #[ignore] #[test] fn anonymous() {}");
    assert_eq!(rows[0]["ignore_reason"], "TOKEN_REASON");
    assert_eq!(rows[1]["ignore_reason"], serde_json::Value::Null);
}

#[test]
fn feature_cfg_does_not_bind_test_to_os() {
    let rows = classify("#[cfg(feature = \"embeddings\")] mod tests { #[test] fn shared() {} }");
    assert_eq!(rows[0]["platform"], false);
}
