use super::*;
use image::codecs::{jpeg::JpegEncoder, png::PngEncoder, webp::WebPEncoder};
use image::{ExtendedColorType, ImageEncoder};

const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">
  <path fill="none" stroke="currentColor" d="M2 2 L30 30"/>
</svg>"#;

fn png(width: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
        .write_image(
            &vec![127; width as usize * 3],
            width,
            1,
            ExtendedColorType::Rgb8,
        )
        .unwrap();
    bytes
}

#[test]
fn chat_wallpaper_refuses_active_svg_and_oversized_sources() {
    for element in [
        r#"<script/>"#,
        r#"<foreignObject/>"#,
        r#"<path href="https://example.invalid/file"/>"#,
        r#"<path onclick="run()"/>"#,
        r#"<path fill="url(#local)"/>"#,
        r#"<path stroke="data:image/svg+xml,malformed"/>"#,
    ] {
        let fixture = format!(r#"<svg viewBox="0 0 32 32">{element}</svg>"#);
        assert!(
            matches!(
                normalize("image/svg+xml", fixture.as_bytes()),
                Err(AssetError::Invalid)
            ),
            "{element}"
        );
    }
    assert!(matches!(
        normalize("image/svg+xml", &vec![b' '; MAX_CHAT_TILE_BYTES + 1]),
        Err(AssetError::TooLarge)
    ));
    assert!(matches!(
        normalize("image/png", &vec![0; UPLOAD_LIMIT + 1]),
        Err(AssetError::TooLarge)
    ));
    assert!(matches!(
        normalize("image/png", &png(4097)),
        Err(AssetError::TooLarge)
    ));
}

#[test]
fn chat_wallpaper_refuses_polyglot_and_mismatched_content() {
    let mut appended = png(2);
    appended.extend_from_slice(b"<!doctype html>");
    assert!(matches!(
        normalize("image/png", &appended),
        Err(AssetError::Invalid)
    ));
    assert!(matches!(
        normalize("image/png", SVG),
        Err(AssetError::Invalid)
    ));
    assert!(matches!(
        normalize("text/html", SVG),
        Err(AssetError::Unsupported)
    ));
}

#[test]
fn chat_wallpaper_reencodes_png_without_metadata() {
    let mut input = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut input, 2, 1);
        encoder.set_color(png::ColorType::Rgb);
        encoder
            .add_text_chunk("Comment".into(), "<html>metadata</html>".into())
            .unwrap();
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[127; 6])
            .unwrap();
    }
    let (_, output) = normalize("image/png", &input).unwrap();
    assert_eq!(
        image::load_from_memory(&output).unwrap().to_rgb8().as_raw(),
        &[127; 6]
    );
    assert_eq!(output, png(2));
    assert_ne!(output, input);
}

#[test]
fn chat_wallpaper_reencodes_jpeg_and_webp_as_static_png() {
    let mut jpeg = Vec::new();
    JpegEncoder::new(&mut jpeg)
        .write_image(&[127; 6], 2, 1, ExtendedColorType::Rgb8)
        .unwrap();
    let mut webp = Vec::new();
    WebPEncoder::new_lossless(&mut webp)
        .write_image(&[127; 6], 2, 1, ExtendedColorType::Rgb8)
        .unwrap();
    for (mime, input) in [("image/jpeg", jpeg), ("image/webp", webp)] {
        let (_, output) = normalize(mime, &input).unwrap();
        assert!(raster::is_canonical_png(&output));
        let decoded = image::load_from_memory(&output).unwrap().to_rgb8();
        assert_eq!(decoded.dimensions(), (2, 1));
        assert_eq!(decoded.as_raw(), &[127; 6]);
        assert_ne!(output, input);
    }
}

#[test]
fn chat_wallpaper_persists_only_content_addressed_normalized_assets() {
    let data = tempfile::tempdir().unwrap();
    let store = WallpaperStore::new(data.path().to_owned());
    let id = store.save("image/svg+xml", SVG).unwrap();
    let reopened = WallpaperStore::new(data.path().to_owned());
    let (mime, bytes) = reopened.read(&id).unwrap();
    assert_eq!(mime, "image/svg+xml");
    assert_eq!(bytes, br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><path fill="none" stroke="currentColor" d="M2 2 L30 30"/></svg>"#);
    assert_eq!(id, asset_name(Kind::Svg, &bytes));
    assert_eq!(store.save("image/svg+xml", &bytes).unwrap(), id);

    let outside = data.path().join("outside.svg");
    fs::write(&outside, SVG).unwrap();
    assert!(matches!(
        store.read("../outside.svg"),
        Err(AssetError::Missing)
    ));
    assert!(matches!(
        store.remove("../outside.svg"),
        Err(AssetError::Missing)
    ));
    assert!(outside.is_file());

    fs::write(data.path().join(DIRECTORY).join(&id), SVG).unwrap();
    assert!(matches!(reopened.read(&id), Err(AssetError::Missing)));
    store.remove(&id).unwrap();
    assert!(matches!(reopened.read(&id), Err(AssetError::Missing)));

    let forged = br#"<svg viewBox="0 0 32 32"><script/></svg>"#;
    let forged_id = asset_name(Kind::Svg, forged);
    fs::write(data.path().join(DIRECTORY).join(&forged_id), forged).unwrap();
    assert!(matches!(
        reopened.read(&forged_id),
        Err(AssetError::Missing)
    ));
}

#[test]
fn chat_wallpaper_copy_requires_canonical_pixels_and_the_exact_content_id() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let source = WallpaperStore::new(source.path().to_owned());
    let target = WallpaperStore::new(target.path().to_owned());
    let id = source.save("image/png", &png(2)).unwrap();
    let (_, normalized) = source.read(&id).unwrap();
    target.copy(&id, &normalized).unwrap();
    assert_eq!(target.read(&id).unwrap().1, normalized);
    let mut modified = normalized;
    modified.extend_from_slice(b"trailing bytes");
    let forged_id = asset_name(Kind::Png, &modified);
    assert!(matches!(
        target.copy(&id, &modified),
        Err(AssetError::Invalid)
    ));
    assert!(matches!(
        target.copy(&forged_id, &modified),
        Err(AssetError::Invalid)
    ));
}
