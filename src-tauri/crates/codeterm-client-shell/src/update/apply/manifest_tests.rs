use super::*;

#[test]
fn retained_metadata_prepares_without_network_and_keeps_signature_verification() {
    let public_key = include_str!(
        "../../../../../../tests/fixtures/auto-update/version-bound/public-key.base64"
    );
    let signature =
        include_str!("../../../../../../tests/fixtures/auto-update/version-bound/signature.base64");
    let payload =
        include_bytes!("../../../../../../tests/fixtures/auto-update/version-bound/payload.txt");
    let mut context = tauri::test::mock_context(tauri::test::noop_assets());
    context.config_mut().plugins.0.insert(
        "updater".into(),
        serde_json::json!({ "pubkey": public_key, "requireSignedVersion": true }),
    );
    let app = crate::app_runtime::builder()
        .plugin(
            tauri_plugin_updater::Builder::new()
                .pubkey(public_key)
                .build(),
        )
        .build(context)
        .unwrap();
    let updater = app
        .updater_builder()
        .target("windows-x86_64")
        .version_comparator(|_, _| true)
        .configure_client(|_| panic!("unexpected metadata or payload request"))
        .build()
        .unwrap();
    let mut manifest: manifest::Manifest = serde_json::from_value(serde_json::json!({
        "version": "2.0.0",
        "platforms": {"windows-x86_64": {
            "url": "https://payload.invalid/retained.exe",
            "signature": signature
        }}
    }))
    .unwrap();
    let update = prepare_update(&updater, &manifest).unwrap().unwrap();
    assert_eq!(update.version, "2.0.0");
    assert_eq!(update.target, "windows-x86_64");
    assert_eq!(
        update.download_url.as_str(),
        "https://payload.invalid/retained.exe"
    );
    assert_eq!(update.signature, signature);
    update.verify(payload).unwrap();
    assert!(update.verify(b"tampered payload").is_err());
    assert!(update.install(b"tampered payload").is_err());
    manifest.version = "3.0.0".into();
    let mismatched = prepare_update(&updater, &manifest).unwrap().unwrap();
    assert!(mismatched.verify(payload).is_err());
}
