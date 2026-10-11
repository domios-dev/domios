use super::*;

#[test]
fn batch_preview_check_excludes_missing_relative_and_directory_paths() {
    let temp = tempfile::tempdir().unwrap();
    let file = preview_test_root(temp.path()).join("notes.md");
    std::fs::write(&file, "# Notes").unwrap();
    let file = file.to_string_lossy().into_owned();
    let result = check_local_preview_files_guarded(
        vec![
            file.clone(),
            preview_test_root(temp.path())
                .join("missing.md")
                .to_string_lossy()
                .into_owned(),
            preview_test_root(temp.path())
                .to_string_lossy()
                .into_owned(),
            "relative.md".into(),
        ],
        |_, _| Ok(()),
    );
    assert_eq!(result.paths, vec![file]);
}

#[test]
fn reads_only_bounded_absolute_html_files() {
    let temp = tempfile::tempdir().unwrap();
    let html = preview_test_root(temp.path()).join("view.html");
    std::fs::write(&html, "<main>safe</main>").unwrap();

    assert_eq!(
        read_local_preview_file_guarded(html.to_str().unwrap(), "page", |_, _| Ok(()))
            .unwrap()
            .data,
        "<main>safe</main>"
    );
    assert!(
        read_local_preview_file_guarded("relative.html", "page", |_, _| Ok(()))
            .unwrap_err()
            .contains("absolute")
    );
}

#[test]
fn rejects_non_html_and_oversized_files() {
    let temp = tempfile::tempdir().unwrap();
    let text = preview_test_root(temp.path()).join("view.txt");
    std::fs::write(&text, "not html").unwrap();
    assert!(
        read_local_preview_file_guarded(text.to_str().unwrap(), "page", |_, _| Ok(()))
            .unwrap_err()
            .contains(".html")
    );

    let oversized = preview_test_root(temp.path()).join("large.html");
    std::fs::write(&oversized, vec![b'x'; HTML_PREVIEW_FILE_LIMIT as usize + 1]).unwrap();
    assert!(
        read_local_preview_file_guarded(oversized.to_str().unwrap(), "page", |_, _| Ok(()))
            .unwrap_err()
            .contains("exceeds")
    );
}

#[test]
fn reads_svg_images_as_svg_data_urls() {
    let temp = tempfile::tempdir().unwrap();
    let svg = preview_test_root(temp.path()).join("qr.svg");
    std::fs::write(&svg, "<svg xmlns='http://www.w3.org/2000/svg'/>").unwrap();

    let image =
        read_local_preview_file_guarded(svg.to_str().unwrap(), "image", |_, _| Ok(())).unwrap();
    assert_eq!(image.media_type, "image/svg+xml");
    assert!(image.data.starts_with("data:image/svg+xml;base64,"));
}

#[test]
fn reads_bounded_documents_and_images_by_explicit_kind() {
    let temp = tempfile::tempdir().unwrap();
    let markdown = preview_test_root(temp.path()).join("notes.md");
    let image = preview_test_root(temp.path()).join("pixel.png");
    std::fs::write(&markdown, "# Notes").unwrap();
    std::fs::write(&image, [0x89, b'P', b'N', b'G']).unwrap();

    let document =
        read_local_preview_file_guarded(markdown.to_str().unwrap(), "document", |_, _| Ok(()))
            .unwrap();
    assert_eq!(document.data, "# Notes");
    assert_eq!(document.media_type, "text/markdown");

    let image =
        read_local_preview_file_guarded(image.to_str().unwrap(), "image", |_, _| Ok(())).unwrap();
    assert_eq!(image.media_type, "image/png");
    assert!(image.data.starts_with("data:image/png;base64,"));
    assert!(
        read_local_preview_file_guarded(markdown.to_str().unwrap(), "image", |_, _| Ok(()))
            .is_err()
    );
}

#[test]
fn reads_source_text_without_an_extension_allowlist() {
    let temp = tempfile::tempdir().unwrap();
    for name in ["current.ts", "module.rs", "Makefile", "unknown.custom"] {
        let path = preview_test_root(temp.path()).join(name);
        let source = "const current = 42;\n";
        std::fs::write(&path, source).unwrap();
        let result =
            read_local_preview_file_guarded(path.to_str().unwrap(), "document", |_, _| Ok(()))
                .unwrap();
        assert_eq!(result.data, source);
        assert_eq!(result.media_type, "text/plain");
    }
}

#[test]
fn rejects_binary_source_text_by_content() {
    let temp = tempfile::tempdir().unwrap();
    let path = preview_test_root(temp.path()).join("binary.ts");
    for bytes in [vec![b'a', 0, b'b'], vec![0xff, 0xfe, 0xfd]] {
        std::fs::write(&path, bytes).unwrap();
        assert!(
            read_local_preview_file_guarded(path.to_str().unwrap(), "document", |_, _| Ok(()))
                .unwrap_err()
                .contains("Binary")
        );
    }
}

#[test]
fn rejects_oversized_source_text() {
    let temp = tempfile::tempdir().unwrap();
    let path = preview_test_root(temp.path()).join("large.ts");
    std::fs::write(&path, vec![b'x'; DOCUMENT_PREVIEW_FILE_LIMIT as usize + 1]).unwrap();
    assert!(
        read_local_preview_file_guarded(path.to_str().unwrap(), "document", |_, _| Ok(()))
            .unwrap_err()
            .contains("exceeds")
    );
}
