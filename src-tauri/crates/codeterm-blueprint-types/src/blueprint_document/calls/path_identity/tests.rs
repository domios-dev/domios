use super::*;

#[test]
fn blueprint_v2_call_path_identity_is_portable() {
    for (source, expected) in [
        (r"\\?\C:\tree\nested\leaf.yaml", "c:/tree/nested/leaf.yaml"),
        (
            r"\\?\UNC\server\share\leaf.yaml",
            "//server/share/leaf.yaml",
        ),
        (r"\\server\share\leaf.yaml", "//server/share/leaf.yaml"),
        (r"C:\tree\leaf.yaml", "c:/tree/leaf.yaml"),
        ("c:/tree/leaf.yaml", "c:/tree/leaf.yaml"),
        ("/tree/nested/leaf.yaml", "/tree/nested/leaf.yaml"),
        (r"nested\leaf.yaml", "nested/leaf.yaml"),
    ] {
        assert_eq!(portable_path(Path::new(source)), expected);
        assert_eq!(portable_path(Path::new(expected)), expected);
    }
}
