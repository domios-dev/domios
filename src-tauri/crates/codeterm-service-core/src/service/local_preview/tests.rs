use super::*;
use crate::agent::chat_types::{ChatAttachment, ChatMsg};
use references::{path_key, referenced_paths};

#[test]
fn references_come_only_from_directives_and_structured_file_records() {
    let temp = tempfile::tempdir().unwrap();
    let root = crate::commands::html_preview::preview_test_root(temp.path());
    let directive = root.join("directive.md");
    let attachment = root.join("attachment.png");
    let diff = root.join("changed.txt");
    let thumbnail = root.join("thumbnail.png");
    let arbitrary = root.join("unreferenced.md");
    let messages = vec![ChatMsg {
        content: format!(
            "Mention {} in prose\n@ds-document \"{}\"",
            arbitrary.display(),
            directive.display()
        ),
        attachments: Some(vec![ChatAttachment {
            path: Some(attachment.to_string_lossy().into()),
            ..Default::default()
        }]),
        file_diffs: Some(serde_json::json!([{ "path": "changed.txt" }])),
        tool_rich_result: Some(serde_json::json!({ "kind": "image", "path": thumbnail })),
        ..Default::default()
    }];
    let paths = referenced_paths(&messages, root.to_str());
    for path in [directive, attachment, diff, thumbnail] {
        assert!(paths.contains(&path_key(&path)));
    }
    assert!(!paths.contains(&path_key(&arbitrary)));
    assert_eq!(paths.len(), 4);
}

#[test]
fn rejects_secret_directories_credential_files_and_configured_roots() {
    let temp = tempfile::tempdir().unwrap();
    let root = crate::commands::html_preview::preview_test_root(temp.path());
    let configured = root.join("private");
    for relative in [
        ".ssh/key.txt",
        ".codeterm-dev/log.txt",
        ".codeterm/token.txt",
        ".aws/config.json",
        ".env.txt",
        "credentials.json",
        "api-token.txt",
        "private/note.md",
    ] {
        let path = root.join(relative);
        let policy =
            PreviewPolicy::new([path_key(&path)].into(), std::slice::from_ref(&configured))
                .unwrap();
        assert!(policy.authorize(&path, None).is_err(), "{relative}");
    }
    let path = root.join("public/note.md");
    let policy = PreviewPolicy::new([path_key(&path)].into(), &[configured]).unwrap();
    assert!(policy.authorize(&path, None).is_ok());
}

#[test]
fn references_do_not_authorize_prefixes_siblings_or_other_tabs() {
    let temp = tempfile::tempdir().unwrap();
    let root = crate::commands::html_preview::preview_test_root(temp.path());
    let own = root.join("own.md");
    let other = root.join("other.md");
    let policy = PreviewPolicy::new([path_key(&own)].into(), &[]).unwrap();
    assert!(policy.authorize(&own, None).is_ok());
    assert!(policy.authorize(&other, None).is_err());
    assert!(policy.authorize(&root.join("own.md.extra"), None).is_err());
    assert!(PreviewPolicy::new(Default::default(), &[])
        .unwrap()
        .authorize(&own, None)
        .is_err());
}

#[test]
fn invalid_configured_secret_paths_fail_closed() {
    assert!(PreviewPolicy::new(Default::default(), &[PathBuf::from("relative")]).is_err());
}

#[test]
fn distinct_case_and_separator_paths_do_not_gain_reference_authority() {
    let temp = tempfile::tempdir().unwrap();
    let root = crate::commands::html_preview::preview_test_root(temp.path());
    let referenced = root.join("Owned.md");
    let other = root.join("owned.md");
    let policy = PreviewPolicy::new([path_key(&referenced)].into(), &[]).unwrap();
    assert!(policy.authorize(&other, None).is_err());
    let secret = root.join(".ssh/key.txt");
    assert!(policy.authorize(&referenced, Some(&secret)).is_err());
}

#[test]
fn directives_inside_fenced_examples_do_not_authorize_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = crate::commands::html_preview::preview_test_root(temp.path());
    let example = root.join("example.md");
    let actual = root.join("actual.md");
    let messages = vec![ChatMsg {
        content: format!(
            "```text\n@ds-document {}\n```\n@ct-document '{}'",
            example.display(),
            actual.display()
        ),
        ..Default::default()
    }];
    assert_eq!(
        referenced_paths(&messages, None),
        [path_key(&actual)].into()
    );
}

#[test]
fn source_text_still_requires_its_own_tab_reference() {
    let temp = tempfile::tempdir().unwrap();
    let root = crate::commands::html_preview::preview_test_root(temp.path());
    let owned = root.join("owned.ts");
    let other = root.join("other.ts");
    std::fs::write(&owned, "const owned = 1;").unwrap();
    std::fs::write(&other, "const other = 2;").unwrap();
    let messages = vec![ChatMsg {
        file_diffs: Some(serde_json::json!([{ "path": "owned.ts" }])),
        ..Default::default()
    }];
    let policy = PreviewPolicy::new(referenced_paths(&messages, root.to_str()), &[]).unwrap();
    let read = |path: &std::path::Path| {
        read_local_preview_file_guarded(path.to_str().unwrap(), "document", |path, canonical| {
            policy.authorize(path, canonical)
        })
    };
    assert_eq!(read(&owned).unwrap().data, "const owned = 1;");
    assert_eq!(read(&other).unwrap_err(), "Preview path is not referenced");
}

#[test]
fn uploaded_artifacts_preview_before_send_without_exposing_private_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = crate::commands::html_preview::preview_test_root(temp.path());
    let private = root.join(".codeterm-dev");
    let uploads = private.join("uploads");
    std::fs::create_dir_all(&uploads).unwrap();
    let image = uploads.join("uploaded.png");
    std::fs::write(&image, b"image").unwrap();
    let token = private.join("token.txt");
    std::fs::write(&token, b"private").unwrap();
    let policy = PreviewPolicy::new(Default::default(), &[private])
        .unwrap()
        .with_uploads(&uploads, &[])
        .unwrap();
    assert!(policy.authorize(&image, None).is_ok());
    assert!(read_local_preview_file_guarded(
        image.to_str().unwrap(),
        "image",
        |path, canonical| policy.authorize(path, canonical)
    )
    .is_ok());
    assert!(policy
        .authorize(&image, Some(&image.canonicalize().unwrap()))
        .is_ok());
    assert!(policy
        .authorize(&token, Some(&token.canonicalize().unwrap()))
        .is_err());
    assert!(policy
        .authorize(&image, Some(&token.canonicalize().unwrap()))
        .is_err());
    assert!(policy
        .authorize(&uploads.join("nested/image.png"), Some(&image))
        .is_err());
    let restricted = PreviewPolicy::new(Default::default(), &[])
        .unwrap()
        .with_uploads(&uploads, &[uploads.clone()])
        .unwrap();
    assert!(restricted
        .authorize(&image, Some(&image.canonicalize().unwrap()))
        .is_err());
}

#[test]
fn inline_markers_authorize_exact_paths_without_routed_or_fenced_prose() {
    let temp = tempfile::tempdir().unwrap();
    let root = crate::commands::html_preview::preview_test_root(temp.path());
    let image = root.join("inline.png");
    std::fs::write(&image, b"image").unwrap();
    let hidden = root.join("hidden.png");
    let messages = vec![ChatMsg {
        content: format!("before <domios type=\"attachment\" kind=\"image\" path=\"{}\" label=\"image1\" /> after\n<domios from=\"tab\" path=\"{}\">\n```\n<domios type=\"attachment\" path=\"{}\" />\n```", image.display(), hidden.display(), hidden.display()),
        ..Default::default()
    }];
    let paths = referenced_paths(&messages, None);
    assert_eq!(paths, [path_key(&image)].into());
    let policy = PreviewPolicy::new(paths, &[]).unwrap();
    assert!(read_local_preview_file_guarded(
        image.to_str().unwrap(),
        "image",
        |path, canonical| policy.authorize(path, canonical)
    )
    .is_ok());
    assert!(policy.authorize(&hidden, None).is_err());
}

#[test]
fn native_attachment_buckets_preview_before_send_with_configured_denials() {
    let temp = tempfile::tempdir().unwrap();
    let root = crate::commands::html_preview::preview_test_root(temp.path());
    let chat = root.join("codeterm-chat");
    let images = root.join("codeterm-images");
    for bucket in [&chat, &images] {
        std::fs::create_dir_all(bucket).unwrap();
    }
    let policy = PreviewPolicy::new(Default::default(), &[])
        .unwrap()
        .with_uploads(&chat, &[])
        .unwrap()
        .with_uploads(&images, &[])
        .unwrap();
    for bucket in [&chat, &images] {
        let image = bucket.join("image.png");
        std::fs::write(&image, b"image").unwrap();
        assert!(read_local_preview_file_guarded(
            image.to_str().unwrap(),
            "image",
            |path, canonical| policy.authorize(path, canonical)
        )
        .is_ok());
    }
    let denied = PreviewPolicy::new(Default::default(), &[])
        .unwrap()
        .with_uploads(&chat, &[chat.clone()])
        .unwrap();
    assert!(denied.authorize(&chat.join("image.png"), None).is_err());
    assert!(policy.authorize(&root.join("other.png"), None).is_err());
}
