use super::*;
use tauri::Assets;

#[test]
fn frontend_shell_contract_development_fallback_has_index_and_module_assets_without_vite() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("assets")).unwrap();
    let html = "<script type=\"module\" src=\"./assets/app.js\"></script>";
    let module = "document.body.dataset.ready = 'true'";
    std::fs::write(root.path().join("index.html"), html).unwrap();
    std::fs::write(root.path().join("assets/app.js"), module).unwrap();
    let assets = DevelopmentAssets::load(root.path()).unwrap();
    std::fs::remove_dir_all(root.path()).unwrap();
    assert_eq!(
        assets.get(&AssetKey::from("index.html")).unwrap().as_ref(),
        html.as_bytes()
    );
    assert_eq!(
        assets
            .get(&AssetKey::from("assets/app.js"))
            .unwrap()
            .as_ref(),
        module.as_bytes()
    );
    assert!(assets.get(&AssetKey::from("../other.js")).is_none());
    assert_eq!(assets.iter().count(), 2);
}

#[test]
fn frontend_shell_contract_development_fallback_refuses_a_missing_index() {
    let root = tempfile::tempdir().unwrap();
    assert!(DevelopmentAssets::load(root.path()).is_err());
}
