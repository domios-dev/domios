use super::*;
use crate::html_preview::read_local_preview_file_guarded;
use codeterm_types::agent::chat_types::{ChatAttachment, ChatMsg};
use references::{path_key, referenced_paths};
use std::path::PathBuf;

#[test]
fn gallery_references_authorize_each_image_without_bypassing_private_path_policy() {
    let temp = tempfile::tempdir().unwrap();
    let root = crate::html_preview::preview_test_root(temp.path());
    let allowed = root.join("allowed image.png");
    let second = root.join("second.png");
    let denied = root.join("private");
    let private_image = denied.join("secret.png");
    std::fs::create_dir(&denied).unwrap();
    std::fs::write(&allowed, b"image").unwrap();
    std::fs::write(&second, b"second").unwrap();
    std::fs::write(&private_image, b"private").unwrap();
    let messages = vec![ChatMsg {
        content: format!(
            "@ds-image \"{}\" \"{}\" \"{}\"",
            allowed.display(),
            second.display(),
            private_image.display()
        ),
        ..Default::default()
    }];
    let references = referenced_paths(&messages, None);
    assert_eq!(
        references,
        [&allowed, &second, &private_image]
            .map(|path| path_key(path))
            .into()
    );
    let policy = PreviewPolicy::new(references, &[denied]).unwrap();
    assert!(policy.authorize(&allowed, Some(&allowed)).is_ok());
    assert!(policy.authorize(&second, Some(&second)).is_ok());
    assert_eq!(
        policy
            .authorize(&private_image, Some(&private_image))
            .unwrap_err(),
        "Preview path is private"
    );
    let read = |path: &std::path::Path| {
        read_local_preview_file_guarded(path.to_str().unwrap(), "image", |path, canonical| {
            policy.authorize(path, canonical)
        })
    };
    assert_eq!(
        read(&allowed).unwrap().data,
        "data:image/png;base64,aW1hZ2U="
    );
    assert_eq!(
        read(&second).unwrap().data,
        "data:image/png;base64,c2Vjb25k"
    );
    assert_eq!(read(&private_image).unwrap_err(), "Preview path is private");
    let combined = PathBuf::from(format!("{} {}", allowed.display(), private_image.display()));
    assert_eq!(
        policy.authorize(&combined, None).unwrap_err(),
        "Preview path is not referenced"
    );
}

#[test]
fn refusal_diagnostics_use_fixed_reasons_without_error_details() {
    for (error, reason) in [
        ("Preview path is private", "private_path"),
        ("Preview path is not referenced", "unreferenced"),
        (
            "Unable to resolve preview file: sensitive-path",
            "resolve_failed",
        ),
        (
            "Unable to open preview file: sensitive-path",
            "secure_open_failed",
        ),
        ("unexpected sensitive-path", "preview_failed"),
    ] {
        assert_eq!(crate::html_preview::refusal_reason(error), reason);
    }
}

#[test]
fn own_pad_policy_allows_only_referenced_nonsecret_descendants() {
    let home = if cfg!(windows) {
        "C:/Users/Preview"
    } else {
        "/home/preview"
    };
    let data = PathBuf::from(home).join(".domios");
    let pad = data.join("runtime/agent-pads/owner");
    let image = pad.join("hush/image.png");
    let document = pad.join("notes/report.md");
    let foreign = data.join("runtime/agent-pads/foreign/image.png");
    let secret = pad.join("secrets/auth.json");
    let store = data.join("secrets/store.json");
    let configured = pad.join("private");
    let configured_file = configured.join("report.md");
    let ordinary = PathBuf::from(home).join("project/report.md");
    let paths = [
        &image,
        &document,
        &foreign,
        &secret,
        &store,
        &configured_file,
        &ordinary,
    ];
    let policy = PreviewPolicy::new(
        paths.into_iter().map(|p| path_key(p)).collect(),
        std::slice::from_ref(&data),
    )
    .unwrap()
    .with_own_pad(&pad, &[configured])
    .unwrap();
    for path in [&image, &document, &ordinary] {
        for raw in [path_key(path), path_key(path).replace('/', "\\")] {
            assert!(
                policy
                    .authorize(std::path::Path::new(&raw), Some(path))
                    .is_ok(),
                "{raw}"
            );
        }
    }
    for path in [
        &foreign,
        &secret,
        &store,
        &configured_file,
        &pad.join("unreferenced.md"),
    ] {
        assert!(
            policy.authorize(path, Some(path)).is_err(),
            "{}",
            path.display()
        );
    }
    assert!(policy.authorize(&image, Some(&foreign)).is_err());
    assert!(policy.authorize(&image, Some(&secret)).is_err());
    #[cfg(windows)]
    assert!(policy
        .authorize(
            std::path::Path::new(&image.to_string_lossy().to_uppercase()),
            Some(&image)
        )
        .is_ok());
}

#[test]
fn references_come_only_from_directives_and_structured_file_records() {
    let temp = tempfile::tempdir().unwrap();
    let root = crate::html_preview::preview_test_root(temp.path());
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
    let root = crate::html_preview::preview_test_root(temp.path());
    let configured = root.join("private");
    for relative in [
        ".ssh/key.txt",
        ".domios-dev/log.txt",
        ".domios/token.txt",
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
    let root = crate::html_preview::preview_test_root(temp.path());
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
    let root = crate::html_preview::preview_test_root(temp.path());
    let referenced = root.join("Owned.md");
    let other = root.join("owned.md");
    let policy = PreviewPolicy::new([path_key(&referenced)].into(), &[]).unwrap();
    #[cfg(windows)]
    assert!(policy.authorize(&other, None).is_ok());
    #[cfg(not(windows))]
    assert!(policy.authorize(&other, None).is_err());
    assert!(policy
        .authorize(&root.join("unreferenced.md"), None)
        .is_err());
    let secret = root.join(".ssh/key.txt");
    assert!(policy.authorize(&referenced, Some(&secret)).is_err());
}

#[test]
fn directives_inside_fenced_examples_do_not_authorize_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = crate::html_preview::preview_test_root(temp.path());
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
    let root = crate::html_preview::preview_test_root(temp.path());
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
    let root = crate::html_preview::preview_test_root(temp.path());
    let private = root.join(".domios-dev");
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
    let root = crate::html_preview::preview_test_root(temp.path());
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
    let root = crate::html_preview::preview_test_root(temp.path());
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

#[test]
fn referenced_evidence_uploads_bypass_only_the_data_root_denial() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let screenshots = data.join("runtime/agent-pads/owner/screenshots");
    std::fs::create_dir_all(&screenshots).unwrap();
    let image = screenshots.join("snip-s1-1.png");
    std::fs::write(&image, b"the captured snip bytes").unwrap();
    let sibling = screenshots.join("unreferenced.png");
    std::fs::write(&sibling, b"other bytes").unwrap();
    let arbitrary = data.join("config.json");
    std::fs::write(&arbitrary, b"private").unwrap();
    let policy = PreviewPolicy::new([path_key(&image)].into(), std::slice::from_ref(&data))
        .unwrap()
        .with_referenced_uploads(&screenshots, &[])
        .unwrap();

    let preview =
        read_local_preview_file_guarded(image.to_str().unwrap(), "image", |path, canonical| {
            policy.authorize(path, canonical)
        })
        .unwrap();
    assert_eq!(
        preview.data,
        "data:image/png;base64,dGhlIGNhcHR1cmVkIHNuaXAgYnl0ZXM="
    );
    assert!(
        policy.authorize(&sibling, None).is_err(),
        "an unreferenced sibling stays unavailable"
    );
    assert!(
        policy.authorize(&arbitrary, None).is_err(),
        "other data-dir files stay private"
    );
}

#[test]
fn shared_context_policy_preserves_upload_and_own_pad_exceptions_and_private_roots() {
    let temp = tempfile::tempdir().unwrap();
    let canonical = temp.path().canonicalize().unwrap();
    let text = canonical.to_string_lossy();
    let root = PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text));
    let data = root.join(".domios");
    let pads = data.join("runtime/agent-pads");
    let own = pads.join("owner");
    let foreign = pads.join("foreign");
    let uploads: Vec<_> = ["uploads", "chat", "images", "files"]
        .map(|bucket| data.join(bucket))
        .into();
    for dir in uploads.iter().chain([&own, &foreign]) {
        std::fs::create_dir_all(dir).unwrap();
    }
    let allowed: Vec<_> = uploads
        .iter()
        .map(|dir| dir.join("screenshot.png"))
        .chain([own.join("report.md")])
        .collect();
    let denied = [
        foreign.join("report.md"),
        own.join(".env"),
        data.join("config.json"),
    ];
    for file in allowed.iter().chain(&denied) {
        std::fs::write(file, b"fixture").unwrap();
    }
    let configured = uploads[0].join("private.png");
    std::fs::write(&configured, b"private").unwrap();
    let paths: Vec<_> = allowed.iter().chain(&denied).chain([&configured]).collect();
    let policy = PreviewPolicy::for_context(
        paths.iter().map(|file| path_key(file)).collect(),
        PreviewRoots {
            configured: vec![configured.clone()],
            private: vec![data, pads],
            uploads,
            own_pad: Some(own),
            ssh: None,
        },
    )
    .unwrap();
    for file in &allowed {
        assert_eq!(
            html_preview::read_artifact_file(file.to_str().unwrap(), &policy, 100).unwrap(),
            b"fixture"
        );
        assert!(read_local_preview_file_guarded(
            file.to_str().unwrap(),
            "document",
            |requested, canonical| policy.authorize(requested, canonical)
        )
        .is_ok());
    }
    for file in denied.iter().chain([&configured]) {
        assert_eq!(
            html_preview::read_artifact_file(file.to_str().unwrap(), &policy, 100).unwrap_err(),
            "Preview path is private"
        );
        assert_eq!(
            read_local_preview_file_guarded(
                file.to_str().unwrap(),
                "document",
                |requested, canonical| policy.authorize(requested, canonical)
            )
            .unwrap_err(),
            "Preview path is private"
        );
    }
}
