use super::*;

#[test]
fn runtime_identity_names_cover_release_and_debug_instances() {
    assert_eq!(
        identity_names_for(false, ""),
        (".domios".into(), "domios".into())
    );
    assert_eq!(
        identity_names_for(true, ""),
        (".domios-dev".into(), "domios-dev".into())
    );
    assert_eq!(
        identity_names_for(true, "-qa-lane"),
        (".domios-dev-qa-lane".into(), "domios-dev-qa-lane".into())
    );
}

#[test]
fn strip_verbatim_prefix_forms() {
    assert_eq!(
        strip_verbatim_prefix(r"\\?\C:\Users\dev\.domios-dev\general"),
        r"C:\Users\dev\.domios-dev\general"
    );
    assert_eq!(
        strip_verbatim_prefix(r"\\?\UNC\server\share\proj"),
        r"\\server\share\proj"
    );
    assert_eq!(
        strip_verbatim_prefix("//?/UNC/server/share/proj"),
        r"\\server\share\proj"
    );
    assert_eq!(
        strip_verbatim_prefix("//?/C:/Users/dev/project"),
        r"C:\Users\dev\project"
    );
    assert_eq!(strip_verbatim_prefix("/home/user/p"), "/home/user/p");
    assert_eq!(strip_verbatim_prefix(r"C:\plain\path"), r"C:\plain\path");
}

#[test]
fn strip_verbatim_prefix_preserves_volume_and_globalroot_paths() {
    let volume = r"\\?\Volume{12345678-1234-1234-1234-123456789abc}\folder";
    let globalroot = r"\\?\GLOBALROOT\Device\HarddiskVolume1\folder";
    assert_eq!(strip_verbatim_prefix(volume), volume);
    assert_eq!(strip_verbatim_prefix(globalroot), globalroot);
    assert_eq!(strip_verbatim_prefix(r"\\?\C:relative"), r"\\?\C:relative");
}

#[test]
fn strip_verbatim_prefix_preserves_extended_length_drive_paths() {
    let tail = "segment".repeat(48);
    let verbatim = format!(r"\\?\C:\{tail}");
    let stripped = strip_verbatim_prefix(&verbatim);
    assert_eq!(stripped, format!(r"C:\{tail}"));
    assert!(verbatim.len() > 260);
}

#[test]
fn strip_verbatim_prefix_normalizes_long_slash_form_paths() {
    let tail = "segment".repeat(48);
    let drive = format!("//?/C:/{tail}");
    let unc = format!("//?/UNC/server/share/{tail}");

    assert_eq!(strip_verbatim_prefix(&drive), format!(r"C:\{tail}"));
    assert_eq!(
        strip_verbatim_prefix(&unc),
        format!(r"\\server\share\{tail}")
    );
    assert!(drive.len() > 260);
    assert!(unc.len() > 260);
}

#[test]
fn canonical_cwd_preserves_case_and_strips_verbatim() {
    let tmp = std::env::temp_dir().to_string_lossy().to_string();
    let out = canonical_cwd(&tmp);
    assert!(!out.starts_with(r"\\?\"), "verbatim prefix leaked: {out}");

    assert_eq!(
        canonical_cwd(r"\\?\C:\Definitely\Missing\Path\"),
        r"C:\Definitely\Missing\Path"
    );
}

#[test]
fn normalize_cwd_ignores_verbatim_drive_and_unc_prefixes() {
    assert_eq!(
        normalize_cwd(r"\\?\C:\lanes\worker"),
        normalize_cwd(r"C:\lanes\worker")
    );
    assert_eq!(
        normalize_cwd(r"\\?\UNC\server\share\lanes"),
        normalize_cwd(r"\\server\share\lanes")
    );
}

#[test]
fn paths_equal_ignores_verbatim_drive_and_unc_prefixes() {
    assert!(paths_equal(r"\\?\C:\lanes\worker\", r"C:/lanes/worker"));
    assert!(paths_equal(
        r"\\?\unc\server\share\lane",
        r"\\server\share\lane"
    ));
}

#[test]
fn explicit_dev_paths_are_named_without_inherited_overrides() {
    assert!(explicit_development_data_dir("qa-lane")
        .unwrap()
        .ends_with(".domios-dev-qa-lane"));
    for name in ["", "../prod", "UPPER", "qa/other", "qa_other"] {
        assert!(explicit_development_data_dir(name).is_err());
    }
}
