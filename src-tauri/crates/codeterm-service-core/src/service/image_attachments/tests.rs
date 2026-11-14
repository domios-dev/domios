use super::*;

#[test]
fn attachment_type_and_size_matrix() {
    for extension in ["jpg", "jpeg", "png", "gif", "webp"] {
        assert_eq!(
            attachment_plan(extension, MODEL_BYTES, Some((2000, 1500)), false),
            ImagePlan::Original
        );
        let expected = if matches!(extension, "gif" | "webp") {
            ImagePlan::SystemJpeg
        } else {
            ImagePlan::Jpeg
        };
        assert_eq!(
            attachment_plan(extension, MODEL_BYTES + 1, Some((2000, 1500)), false),
            expected
        );
        assert_eq!(
            attachment_plan(extension, 100, Some((2001, 1500)), false),
            expected
        );
        assert_eq!(
            attachment_plan(extension, 100, Some((0, 1)), false),
            ImagePlan::Refuse
        );
        assert_eq!(
            attachment_plan(extension, 100, Some((16000, 16000)), false),
            ImagePlan::Refuse
        );
    }
    for extension in [
        "heic", "heif", "avif", "tif", "tiff", "bmp", "dng", "raw", "cr2", "cr3", "nef", "arw",
        "orf", "rw2", "raf", "pef", "srw",
    ] {
        assert_eq!(
            attachment_plan(extension, 25 * 1024 * 1024, None, false),
            ImagePlan::SystemJpeg
        );
    }
    for extension in ["gif", "webp"] {
        assert_eq!(
            attachment_plan(extension, 100, Some((100, 100)), true),
            ImagePlan::SystemJpeg
        );
    }
    for extension in [
        "svg", "mov", "mp4", "webm", "mp3", "wav", "m4a", "pdf", "zip", "docx", "xlsx", "pptx",
        "rs", "ts", "txt", "bin", "unknown",
    ] {
        assert_eq!(
            attachment_plan(extension, 100 * 1024 * 1024, None, false),
            ImagePlan::Path
        );
        assert_eq!(
            attachment_plan(extension, MAX_SOURCE_BYTES, None, false),
            ImagePlan::Path
        );
    }
    assert_eq!(
        attachment_plan("jpg", MAX_SOURCE_BYTES + 1, None, false),
        ImagePlan::Refuse
    );
    assert_eq!(attachment_plan("jpg", 0, None, false), ImagePlan::Refuse);
}

#[test]
fn large_image_keeps_original_and_delivers_a_bounded_jpeg() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("phone.png");
    let image = image::RgbImage::from_pixel(2200, 20, image::Rgb([5, 30, 200]));
    image.save(&source).unwrap();
    let original = std::fs::read(&source).unwrap();
    let text = format!("before {} after", marker("image", &source, Some("image1")));
    let prepared = prepare(text, || Ok(dir.path().to_path_buf())).unwrap();
    let segments = codeterm_wire::envelope::parse_envelope(&prepared);
    let paths: Vec<_> = segments
        .into_iter()
        .filter_map(|segment| match segment {
            MessageSegment::Attachment { kind, path, .. } => Some((kind, path)),
            _ => None,
        })
        .collect();
    assert_eq!(paths.len(), 2);
    assert_eq!(
        paths[0],
        ("file".into(), source.to_string_lossy().into_owned())
    );
    assert_eq!(paths[1].0, "image");
    let model = Path::new(&paths[1].1);
    assert_eq!(
        ImageReader::open(model).unwrap().into_dimensions().unwrap(),
        (2000, 18)
    );
    assert!(std::fs::metadata(model).unwrap().len() <= MODEL_BYTES);
    assert_eq!(std::fs::read(source).unwrap(), original);
    assert!(prepared.starts_with("before ") && prepared.ends_with(" after"));
    let thumbnail = preview(Path::new("phone.png"), || Ok(original)).unwrap();
    assert_eq!(thumbnail.media_type, "image/jpeg");
    assert!(thumbnail.data.starts_with("data:image/jpeg;base64,"));
}

#[test]
fn path_only_types_do_not_open_or_decode_files() {
    for extension in ["mov", "pdf", "zip", "bin", "svg"] {
        let input = marker(
            "file",
            Path::new(&format!("/missing/file.{extension}")),
            None,
        );
        assert_eq!(
            prepare(input.clone(), || panic!(
                "must not allocate preview storage"
            ))
            .unwrap(),
            input
        );
        assert!(
            preview(Path::new(&format!("/missing/file.{extension}")), || panic!(
                "must not load bytes for a file chip"
            ))
            .is_err()
        );
    }
    let svg = marker("image", Path::new("/missing/vector.svg"), Some("image1"));
    let output = prepare(svg, || panic!("must not decode SVG")).unwrap();
    assert!(output.contains("kind=\"file\""));
    let quoted = "`<domios type=\"attachment\" kind=\"image\" path=\"/missing/phone.jpg\" />`";
    assert_eq!(
        prepare(quoted.into(), || panic!("quoted examples are not files")).unwrap(),
        quoted
    );
}

#[test]
fn gif_metadata_distinguishes_a_single_image_from_animation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("image.gif");
    let mut header = b"GIF89a".to_vec();
    header.extend_from_slice(&[1, 0, 1, 0, 0, 0, 0]);
    let frame = [0x2c, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0];
    for frames in [1, 2] {
        let mut bytes = header.clone();
        for _ in 0..frames {
            bytes.extend_from_slice(&frame);
        }
        bytes.push(0x3b);
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(headers::inspect(&path, "gif"), Some(((1, 1), frames > 1)));
    }
}

#[cfg(not(target_os = "macos"))]
#[test]
fn unavailable_camera_decoder_delivers_original_with_an_explanation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("phone.HEIC");
    std::fs::write(&path, b"opaque-camera-original").unwrap();
    let output = prepare(marker("file", &path, None), || Ok(dir.path().to_path_buf())).unwrap();
    assert!(output.contains("kind=\"file\""));
    assert!(output.contains("Model preview unavailable"));
    assert_eq!(std::fs::read(path).unwrap(), b"opaque-camera-original");
}
