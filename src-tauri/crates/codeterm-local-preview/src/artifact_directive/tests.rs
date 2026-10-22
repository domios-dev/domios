use super::artifact_paths;

#[test]
fn image_gallery_paths_support_quotes_mixed_paths_and_all_host_path_forms() {
    assert_eq!(
        artifact_paths(
            "@ds-image",
            "\"/first image.png\" /second.png 'C:\\third image.png' \\\\host\\fourth.png"
        ),
        [
            "/first image.png",
            "/second.png",
            "C:\\third image.png",
            "\\\\host\\fourth.png"
        ]
    );
    assert_eq!(
        artifact_paths("@CT-IMAGE", "/owner's.png /two.png"),
        ["/owner's.png", "/two.png"]
    );
}

#[test]
fn single_image_paths_keep_the_existing_quoting_and_space_contract() {
    for (argument, expected) in [
        ("/work/first image.png", "/work/first image.png"),
        ("\"C:\\work\\a b.png\"", "C:\\work\\a b.png"),
        ("'/work/one.png'", "/work/one.png"),
        ("/work/owner's image.png", "/work/owner's image.png"),
        ("relative image.png", "relative image.png"),
    ] {
        assert_eq!(artifact_paths("@ds-image", argument), [expected]);
    }
    assert!(artifact_paths("@ds-image", "\"unfinished").is_empty());
    assert!(artifact_paths("@ds-image", "").is_empty());
}

#[test]
fn other_artifacts_still_have_one_path() {
    assert_eq!(
        artifact_paths("@ds-document", "/a.md /b.md"),
        ["/a.md /b.md"]
    );
    assert_eq!(
        artifact_paths("@file", "\"/page with spaces.html\""),
        ["/page with spaces.html"]
    );
}
