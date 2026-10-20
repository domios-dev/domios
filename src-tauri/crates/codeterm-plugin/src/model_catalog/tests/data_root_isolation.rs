use super::*;

#[test]
fn model_cache_writes_land_in_the_isolated_temp_root_never_the_user_root() {
    let id = format!("endpoint-overlay-guard-{}", uuid::Uuid::new_v4());
    put_live(
        &id,
        vec![ModelInfo {
            id: "guard".into(),
            ..Default::default()
        }],
        None,
    );
    let written = cache_path(&id);
    let user_root = dirs::home_dir()
        .expect("home directory")
        .join(codeterm_paths::dir_name());

    assert!(written.is_file());
    assert!(written.starts_with(std::env::temp_dir()));
    assert!(!written.starts_with(&user_root));
    assert!(!user_root
        .join("models")
        .join(written.file_name().unwrap())
        .exists());
    cleanup(&id);
}
