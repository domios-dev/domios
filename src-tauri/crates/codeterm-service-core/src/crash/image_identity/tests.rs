use super::*;

/// Build an RSDS CodeView record the way a linker writes one.
fn rsds(guid: [u8; 16], age: u32, pdb: &str) -> Vec<u8> {
    let mut record = Vec::new();
    record.extend_from_slice(b"RSDS");
    record.extend_from_slice(&guid);
    record.extend_from_slice(&age.to_le_bytes());
    record.extend_from_slice(pdb.as_bytes());
    record.push(0);
    record
}

/// The GUID the 2026-09-05 dump recorded for codeterm.exe, byte-for-byte:
/// {4B469720-F59E-45EF-97AB-215696E6BF03}.
const CRASHED_BUILD_GUID: [u8; 16] = [
    0x20, 0x97, 0x46, 0x4B, // Data1, little-endian
    0x9E, 0xF5, // Data2
    0xEF, 0x45, // Data3
    0x97, 0xAB, 0x21, 0x56, 0x96, 0xE6, 0xBF, 0x03, // Data4, as stored
];

#[test]
fn a_supplied_rsds_round_trips_to_the_identity_the_sidecar_records() {
    let rendered = render_rsds(&rsds(CRASHED_BUILD_GUID, 1, "codeterm.pdb"))
        .expect("a well-formed RSDS record renders");

    assert_eq!(
        rendered,
        "4B469720-F59E-45EF-97AB-215696E6BF03 age=1 pdb=codeterm.pdb"
    );
}

#[test]
fn the_age_and_pdb_name_are_carried_through_not_hardcoded() {
    let rendered = render_rsds(&rsds([0xAB; 16], 7, "other.pdb")).expect("renders");
    assert!(rendered.contains("age=7"), "{rendered}");
    assert!(rendered.contains("pdb=other.pdb"), "{rendered}");
    assert!(!rendered.contains("age=1"), "{rendered}");
}

#[test]
fn a_pdb_path_is_reduced_to_its_file_name() {
    // MSVC records an absolute build-machine path; only the name is a lookup key.
    let rendered = render_rsds(&rsds(
        CRASHED_BUILD_GUID,
        1,
        r"D:\build\target\codeterm.pdb",
    ))
    .expect("renders");
    assert!(rendered.ends_with("pdb=codeterm.pdb"), "{rendered}");
    assert!(!rendered.contains("D:"), "{rendered}");
}

#[test]
fn two_builds_never_render_the_same_identity() {
    let first = render_rsds(&rsds(CRASHED_BUILD_GUID, 1, "codeterm.pdb")).expect("renders");
    let mut other_guid = CRASHED_BUILD_GUID;
    other_guid[0] ^= 0xFF;
    let second = render_rsds(&rsds(other_guid, 1, "codeterm.pdb")).expect("renders");
    assert_ne!(
        first, second,
        "the identity must separate builds, or it is not a lookup key"
    );
}

#[test]
fn a_record_that_is_not_rsds_yields_no_identity_rather_than_a_guess() {
    assert!(render_rsds(b"NB10\x00\x00\x00\x00").is_none());
    assert!(render_rsds(b"").is_none());
    assert!(render_rsds(&rsds(CRASHED_BUILD_GUID, 1, "x.pdb")[0..20]).is_none());
}

#[test]
fn a_build_id_renders_as_uppercase_hex() {
    assert_eq!(render_hex_id(&[0x0a, 0xff, 0x10]), "0AFF10");
    assert_eq!(render_hex_id(&[]), "");
}

#[test]
fn the_running_image_reports_an_identity_or_says_it_has_none() {
    // Never empty: the sidecar field must always be present and self-describing.
    let id = image_debug_id();
    assert!(!id.is_empty());
    assert!(!image_path().is_empty());
    if id != IDENTITY_UNAVAILABLE {
        assert!(
            id.contains("age=") || id.starts_with("build-id=") || id.starts_with("uuid="),
            "unexpected identity shape: {id}"
        );
    }
}

#[test]
fn the_key_is_the_symbol_store_form_of_the_same_record() {
    // scripts/pdb_identity.py prints exactly this for the PDB of that build; the
    // two must agree or the sidecar names a directory retention never wrote.
    let key = rsds_key(&rsds(CRASHED_BUILD_GUID, 1, "codeterm.pdb")).expect("renders");
    assert_eq!(key, "4B469720F59E45EF97AB215696E6BF031");
    assert_eq!(key.len(), 33, "32 GUID hex digits then the age in hex");
    assert!(!key.contains('-'), "the key carries no separators: {key}");
}

#[test]
fn the_key_separates_builds_that_differ_only_in_age() {
    let first = rsds_key(&rsds(CRASHED_BUILD_GUID, 1, "codeterm.pdb")).expect("renders");
    let second = rsds_key(&rsds(CRASHED_BUILD_GUID, 2, "codeterm.pdb")).expect("renders");
    assert_ne!(first, second);
}

#[test]
fn a_record_with_no_identity_has_no_key() {
    assert!(rsds_key(b"NB10").is_none());
}

#[test]
fn the_retained_symbols_dir_is_keyed_by_identity_not_by_a_commit() {
    let dir = crate::crash::report::retained_symbols_dir();
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    assert!(dir.parent().is_some_and(|p| p.ends_with("pdb")) || name == "pdb");
    if let Some(key) = debug_key() {
        assert_eq!(
            name, key,
            "the sidecar must name the directory retention writes"
        );
        assert_ne!(
            name,
            env!("BUILD_HASH"),
            "a commit is not a symbol identity"
        );
    }
}
