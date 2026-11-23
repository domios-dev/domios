use image::codecs::{png::PngEncoder, webp::WebPDecoder};
use image::{DynamicImage, ImageDecoder, ImageEncoder, ImageFormat, ImageReader, Limits};
use std::io::{self, Cursor, Write};

use super::{AssetError, ASSET_LIMIT};

const MAX_EDGE: u32 = 4096;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

pub(super) fn normalize(media_type: &str, input: &[u8]) -> Result<Vec<u8>, AssetError> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_EDGE);
    limits.max_image_height = Some(MAX_EDGE);
    limits.max_alloc = Some(160 * 1024 * 1024);

    let filtered;
    let mut decoder: Box<dyn ImageDecoder> = match media_type {
        "image/png" => {
            filtered = pixel_chunks(input, false).ok_or(AssetError::Invalid)?;
            let mut reader = ImageReader::with_format(Cursor::new(&filtered), ImageFormat::Png);
            reader.limits(limits.clone());
            Box::new(reader.into_decoder().map_err(decode_error)?)
        }
        "image/jpeg" if complete_jpeg(input) => {
            let mut reader = ImageReader::with_format(Cursor::new(input), ImageFormat::Jpeg);
            reader.limits(limits.clone());
            Box::new(reader.into_decoder().map_err(decode_error)?)
        }
        "image/webp" if complete_webp(input) => {
            let decoder = WebPDecoder::new(Cursor::new(input)).map_err(decode_error)?;
            if decoder.has_animation() {
                return Err(AssetError::Invalid);
            }
            Box::new(decoder)
        }
        _ => return Err(AssetError::Invalid),
    };
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 {
        return Err(AssetError::Invalid);
    }
    limits
        .check_dimensions(width, height)
        .map_err(decode_error)?;
    limits
        .reserve(decoder.total_bytes())
        .map_err(decode_error)?;
    decoder.set_limits(limits).map_err(decode_error)?;
    let orientation = decoder.orientation().map_err(decode_error)?;
    let mut decoded = DynamicImage::from_decoder(decoder).map_err(decode_error)?;
    decoded.apply_orientation(orientation);

    let pixels = if decoded.color().has_alpha() {
        DynamicImage::ImageRgba8(decoded.into_rgba8())
    } else {
        DynamicImage::ImageRgb8(decoded.into_rgb8())
    };
    let mut output = BoundedPng(Vec::new());
    PngEncoder::new(&mut output)
        .write_image(
            pixels.as_bytes(),
            pixels.width(),
            pixels.height(),
            pixels.color().into(),
        )
        .map_err(|error| match error {
            image::ImageError::IoError(error) if error.kind() == io::ErrorKind::FileTooLarge => {
                AssetError::TooLarge
            }
            _ => AssetError::Invalid,
        })?;
    Ok(output.0)
}

fn decode_error(error: image::ImageError) -> AssetError {
    match error {
        image::ImageError::Limits(_) => AssetError::TooLarge,
        _ => AssetError::Invalid,
    }
}

struct BoundedPng(Vec<u8>);

impl Write for BoundedPng {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > ASSET_LIMIT - self.0.len() {
            return Err(io::ErrorKind::FileTooLarge.into());
        }
        self.0.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// Remove ancillary input before decoding so compressed metadata cannot allocate memory.
fn pixel_chunks(input: &[u8], canonical: bool) -> Option<Vec<u8>> {
    let mut tail = input.strip_prefix(PNG_SIGNATURE)?;
    let mut pixels = if canonical {
        Vec::new()
    } else {
        PNG_SIGNATURE.to_vec()
    };
    let mut first = true;
    let mut saw_pixels = false;
    while let Some(header) = tail.first_chunk::<8>() {
        let size = u32::from_be_bytes(header[..4].try_into().ok()?) as usize;
        let chunk = tail.get(..size.checked_add(12)?)?;
        tail = tail.get(chunk.len()..)?;
        let name = &header[4..];
        if first != (name == b"IHDR") {
            return None;
        }
        first = false;
        match name {
            b"IHDR" if size == 13 => {}
            b"IDAT" => saw_pixels = true,
            b"IEND" if size == 0 && saw_pixels && tail.is_empty() => {
                if !canonical {
                    pixels.extend_from_slice(chunk);
                }
                return Some(pixels);
            }
            b"PLTE" | b"tRNS" if !canonical => {}
            b"acTL" | b"fcTL" | b"fdAT" => return None,
            _ if !canonical && name[0].is_ascii_lowercase() => continue,
            _ => return None,
        }
        if !canonical {
            pixels.extend_from_slice(chunk);
        }
    }
    None
}

pub(super) fn is_canonical_png(bytes: &[u8]) -> bool {
    pixel_chunks(bytes, true).is_some()
}

fn complete_webp(bytes: &[u8]) -> bool {
    let Some(header) = bytes.first_chunk::<12>() else {
        return false;
    };
    header[..4] == *b"RIFF"
        && header[8..] == *b"WEBP"
        && header[4..8]
            .try_into()
            .is_ok_and(|size| u32::from_le_bytes(size) as usize == bytes.len() - 8)
}

fn complete_jpeg(bytes: &[u8]) -> bool {
    let Some(mut tail) = bytes.strip_prefix(&[0xff, 0xd8]) else {
        return false;
    };
    let mut in_scan = false;
    loop {
        if in_scan {
            let Some(prefix) = tail.iter().position(|&byte| byte == 0xff) else {
                return false;
            };
            tail = &tail[prefix..];
        }
        let Some(rest) = tail.strip_prefix(&[0xff]) else {
            return false;
        };
        let Some(marker_offset) = rest.iter().position(|&byte| byte != 0xff) else {
            return false;
        };
        let marker = rest[marker_offset];
        tail = &rest[marker_offset + 1..];
        match marker {
            0xd9 => return tail.is_empty(),
            0x00 | 0xd0..=0xd7 if in_scan => continue,
            0x00 | 0xd0..=0xd8 => return false,
            _ => {}
        }
        let Some(size) = tail
            .first_chunk::<2>()
            .map(|bytes| u16::from_be_bytes(*bytes))
        else {
            return false;
        };
        if size < 2 || usize::from(size) > tail.len() {
            return false;
        }
        tail = &tail[usize::from(size)..];
        in_scan = marker == 0xda;
    }
}
