use codeterm_wire::{
    domios::build_opening_tag,
    envelope::{parse_envelope_spans, MessageSegment},
};
use image::ImageReader;
use std::path::{Path, PathBuf};

mod headers;

const MODEL_EDGE: u32 = 2000;
const MODEL_BYTES: u64 = 4 * 1024 * 1024;
const MAX_SOURCE_BYTES: u64 = 150 * 1024 * 1024;
const MAX_DECODE_PIXELS: u64 = 100_000_000;

#[derive(Debug, PartialEq, Eq)]
enum ImagePlan {
    Path,
    Inspect,
    Original,
    Jpeg,
    SystemJpeg,
    Refuse,
}

fn attachment_plan(
    extension: &str,
    bytes: u64,
    dimensions: Option<(u32, u32)>,
    animated: bool,
) -> ImagePlan {
    if bytes == 0 || bytes > MAX_SOURCE_BYTES {
        return ImagePlan::Refuse;
    }
    let native = matches!(extension, "jpg" | "jpeg" | "png");
    let direct = native || matches!(extension, "gif" | "webp");
    let camera = matches!(
        extension,
        "heic"
            | "heif"
            | "avif"
            | "tif"
            | "tiff"
            | "bmp"
            | "dng"
            | "raw"
            | "cr2"
            | "cr3"
            | "nef"
            | "arw"
            | "orf"
            | "rw2"
            | "raf"
            | "pef"
            | "srw"
    );
    if !direct && !camera {
        return ImagePlan::Path;
    }
    if camera || animated {
        return ImagePlan::SystemJpeg;
    }
    let Some((width, height)) = dimensions else {
        return if native {
            ImagePlan::Inspect
        } else {
            ImagePlan::SystemJpeg
        };
    };
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_DECODE_PIXELS {
        return ImagePlan::Refuse;
    }
    if bytes > MODEL_BYTES || width > MODEL_EDGE || height > MODEL_EDGE {
        if native {
            ImagePlan::Jpeg
        } else {
            ImagePlan::SystemJpeg
        }
    } else {
        ImagePlan::Original
    }
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn marker(kind: &str, path: &Path, label: Option<&str>) -> String {
    let path = path.to_string_lossy();
    let mut attrs = vec![
        ("type", "attachment"),
        ("kind", kind),
        ("path", path.as_ref()),
    ];
    if let Some(label) = label {
        attrs.push(("label", label));
    }
    let tag = build_opening_tag(&attrs);
    format!("{} />", tag.trim_end_matches('>'))
}

pub(super) fn preview(
    source: &Path,
    load: impl FnOnce() -> Result<Vec<u8>, String>,
) -> Result<codeterm_wire::api_contracts::LocalPreviewFile, String> {
    use base64::Engine;
    use std::io::Cursor;
    if !matches!(extension(source).as_str(), "jpg" | "jpeg" | "png") {
        return Err("A host thumbnail is unavailable for this format; use the file path".into());
    }
    let bytes = load()?;
    let reader = || {
        ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|error| error.to_string())
    };
    let dimensions = reader()?
        .into_dimensions()
        .map_err(|error| error.to_string())?;
    if attachment_plan(
        &extension(source),
        bytes.len() as u64,
        Some(dimensions),
        false,
    ) == ImagePlan::Refuse
    {
        return Err("Image dimensions exceed the safe decoding limit".into());
    }
    let decoded = reader()?.decode().map_err(|error| error.to_string())?;
    let mut jpeg = Vec::new();
    encode_thumbnail(decoded, &mut jpeg)?;
    if jpeg.len() as u64 > MODEL_BYTES {
        return Err("Image thumbnail exceeds its size limit".into());
    }
    Ok(codeterm_wire::api_contracts::LocalPreviewFile {
        data: format!(
            "data:image/jpeg;base64,{}",
            base64::prelude::BASE64_STANDARD.encode(jpeg)
        ),
        media_type: "image/jpeg".into(),
    })
}

fn encode_thumbnail(
    decoded: image::DynamicImage,
    output: impl std::io::Write,
) -> Result<(), String> {
    let rgb = decoded.thumbnail(MODEL_EDGE, MODEL_EDGE).to_rgb8();
    image::codecs::jpeg::JpegEncoder::new_with_quality(output, 80)
        .encode_image(&rgb)
        .map_err(|error| error.to_string())
}

fn jpeg_copy(source: &Path, destination: &Path, bytes: u64) -> Result<Option<PathBuf>, String> {
    let extension = extension(source);
    let header = headers::inspect(source, &extension);
    match attachment_plan(
        &extension,
        bytes,
        header.map(|value| value.0),
        header.is_some_and(|value| value.1),
    ) {
        ImagePlan::Path | ImagePlan::Original => return Ok(None),
        ImagePlan::SystemJpeg => return system_copy(source, destination).map(Some),
        ImagePlan::Refuse => return Err("Attachment exceeds the safe image size limit".into()),
        _ => {}
    }
    let reader = || {
        ImageReader::open(source)
            .map_err(|error| error.to_string())?
            .with_guessed_format()
            .map_err(|error| error.to_string())
    };
    let dimensions = reader()?
        .into_dimensions()
        .map_err(|error| format!("Cannot read image dimensions: {error}"))?;
    match attachment_plan(&extension, bytes, Some(dimensions), false) {
        ImagePlan::Original => return Ok(None),
        ImagePlan::Refuse => return Err("Image dimensions exceed the safe decoding limit".into()),
        _ => {}
    }
    let mut decoded = reader()?.decode().map_err(|error| error.to_string())?;
    let orientation = reader()?.into_decoder().ok().and_then(|mut decoder| {
        use image::ImageDecoder;
        decoder.orientation().ok()
    });
    if let Some(orientation) = orientation {
        decoded.apply_orientation(orientation);
    }
    let mut file =
        tempfile::NamedTempFile::new_in(destination).map_err(|error| error.to_string())?;
    encode_thumbnail(decoded, file.as_file_mut())?;
    if file
        .as_file()
        .metadata()
        .map_err(|error| error.to_string())?
        .len()
        > MODEL_BYTES
    {
        return Err("The model-sized JPEG exceeds 4 MiB".into());
    }
    let target = destination.join(format!("{}-model.jpg", uuid::Uuid::new_v4().simple()));
    file.persist(&target).map_err(|error| error.to_string())?;
    Ok(Some(target))
}

#[cfg(target_os = "macos")]
fn system_copy(source: &Path, destination: &Path) -> Result<PathBuf, String> {
    let output = tempfile::Builder::new()
        .suffix(".jpg")
        .tempfile_in(destination)
        .map_err(|error| error.to_string())?;
    let mut command = std::process::Command::new("/usr/bin/sips");
    command
        .args([
            "-s",
            "format",
            "jpeg",
            "-s",
            "formatOptions",
            "80",
            "-Z",
            "2000",
        ])
        .arg(source)
        .arg("--out")
        .arg(output.path());
    let result =
        codeterm_util::process_util::bounded_output(command, std::time::Duration::from_secs(60))
            .map_err(|error| format!("macOS image conversion failed: {error}"))?;
    if !result.status.success() {
        return Err("macOS could not convert this image".into());
    }
    let size = std::fs::metadata(output.path())
        .map_err(|error| error.to_string())?
        .len();
    let dimensions = ImageReader::open(output.path())
        .map_err(|error| error.to_string())?
        .with_guessed_format()
        .map_err(|error| error.to_string())?
        .into_dimensions()
        .map_err(|error| error.to_string())?;
    if attachment_plan("jpg", size, Some(dimensions), false) != ImagePlan::Original {
        return Err("macOS did not produce a model-sized JPEG".into());
    }
    let target = destination.join(format!("{}-model.jpg", uuid::Uuid::new_v4().simple()));
    output.persist(&target).map_err(|error| error.to_string())?;
    Ok(target)
}

#[cfg(not(target_os = "macos"))]
fn system_copy(_source: &Path, _destination: &Path) -> Result<PathBuf, String> {
    Err("This host cannot create a model preview for this image format; use the original file path or attach a JPEG or PNG".into())
}

pub(super) fn prepare(
    text: String,
    destination: impl Fn() -> Result<PathBuf, String>,
) -> Result<String, String> {
    let mut result = String::new();
    let mut cursor = 0;
    for item in parse_envelope_spans(&text) {
        let MessageSegment::Attachment {
            kind, path, label, ..
        } = item.segment
        else {
            continue;
        };
        let source = Path::new(&path);
        if !matches!(kind.as_str(), "image" | "file") {
            continue;
        }
        let plan = attachment_plan(&extension(source), 1, None, false);
        if plan == ImagePlan::Path {
            if kind == "image" {
                result.push_str(&text[cursor..item.span.start]);
                result.push_str(&marker("file", source, label.as_deref()));
                cursor = item.span.end;
            }
            continue;
        }
        let size = crate::attachments::validate_complete_file(source, MAX_SOURCE_BYTES)?;
        let converted = jpeg_copy(source, &destination()?, size);
        #[cfg(not(target_os = "macos"))]
        if let Err(error) = &converted {
            if plan == ImagePlan::SystemJpeg {
                result.push_str(&text[cursor..item.span.start]);
                result.push_str(&marker("file", source, label.as_deref()));
                result.push_str(&format!(
                    " [Model preview unavailable: {error}. Original kept.]"
                ));
                cursor = item.span.end;
                continue;
            }
        }
        let converted = converted
            .map_err(|error| format!("Image was not sent: {error}. Original kept at {path}"))?;
        let Some(model) = converted else {
            continue;
        };
        result.push_str(&text[cursor..item.span.start]);
        result.push_str(&marker("file", source, Some("original")));
        result.push(' ');
        result.push_str(&marker("image", &model, label.as_deref()));
        cursor = item.span.end;
    }
    if cursor == 0 {
        return Ok(text);
    }
    result.push_str(&text[cursor..]);
    Ok(result)
}

#[cfg(test)]
mod tests;
