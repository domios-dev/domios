use super::MediaType;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};
const MAX_HEADER_BYTES: usize = 256 * 1024;
pub const MAX_IMAGE_PREVIEW_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_IMAGE_PREVIEW_EDGE: u32 = 8192;
pub const MAX_IMAGE_PREVIEW_PIXELS: u64 = 16 * 1024 * 1024;

pub(super) fn allowed(file: &mut File, size: u64, kind: MediaType) -> bool {
    snapshot(file, size, kind).is_some()
}
pub(super) fn snapshot(file: &mut File, size: u64, kind: MediaType) -> Option<Vec<u8>> {
    if !kind.image()
        || size == 0
        || size > MAX_IMAGE_PREVIEW_BYTES
        || !file
            .metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() >= size)
    {
        return None;
    }
    let budget = match kind {
        MediaType::Jpeg => MAX_HEADER_BYTES,
        MediaType::Png => 33,
        MediaType::Gif => 13,
        MediaType::Webp => 30,
        _ => return None,
    };
    let mut header = vec![0; size.min(budget as u64) as usize];
    let read = file
        .seek(SeekFrom::Start(0))
        .and_then(|_| file.read_exact(&mut header));
    let reset = file.seek(SeekFrom::Start(0));
    if read.is_err() || reset.is_err() {
        return None;
    }
    let (width, height) = dimensions(kind, &header, size)?;
    (width > 0
        && height > 0
        && width <= MAX_IMAGE_PREVIEW_EDGE
        && height <= MAX_IMAGE_PREVIEW_EDGE
        && u64::from(width) * u64::from(height) <= MAX_IMAGE_PREVIEW_PIXELS)
        .then_some(header)
}
fn dimensions(kind: MediaType, bytes: &[u8], size: u64) -> Option<(u32, u32)> {
    if !kind.matches(bytes) {
        return None;
    }
    match kind {
        MediaType::Png => {
            if bytes.len() < 33 || bytes.get(8..16)? != b"\0\0\0\x0dIHDR" {
                return None;
            }
            Some((be32(bytes, 16)?, be32(bytes, 20)?))
        }
        MediaType::Gif => {
            if bytes.len() < 13 {
                return None;
            }
            Some((u32::from(le16(bytes, 6)?), u32::from(le16(bytes, 8)?)))
        }
        MediaType::Jpeg => jpeg(bytes),
        MediaType::Webp => webp(bytes, size),
        _ => None,
    }
}
fn be16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]))
}
fn le16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]))
}
fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}
fn le32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}
fn le24(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *bytes.get(at)?,
        *bytes.get(at + 1)?,
        *bytes.get(at + 2)?,
        0,
    ]))
}
fn jpeg(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut cursor = 2usize;
    for _ in 0..512 {
        if *bytes.get(cursor)? != 0xff {
            return None;
        }
        while *bytes.get(cursor)? == 0xff {
            cursor += 1;
        }
        let marker = *bytes.get(cursor)?;
        cursor += 1;
        if matches!(marker, 0 | 0x01 | 0xd0..=0xd9 | 0xda) {
            return None;
        }
        let length = usize::from(be16(bytes, cursor)?);
        if length < 2 {
            return None;
        }
        let end = cursor.checked_add(length)?;
        if end > bytes.len() {
            return None;
        }
        if matches!(marker, 0xc0 | 0xc1 | 0xc2) {
            let components = usize::from(*bytes.get(cursor + 7)?);
            if *bytes.get(cursor + 2)? != 8
                || !(1..=4).contains(&components)
                || length != 8 + 3 * components
            {
                return None;
            }
            return Some((
                u32::from(be16(bytes, cursor + 5)?),
                u32::from(be16(bytes, cursor + 3)?),
            ));
        }
        if matches!(marker,0xc3|0xc5..=0xc7|0xc9..=0xcb|0xcd..=0xcf) {
            return None;
        }
        cursor = end;
    }
    None
}
fn webp(bytes: &[u8], size: u64) -> Option<(u32, u32)> {
    if u64::from(le32(bytes, 4)?).checked_add(8)? != size {
        return None;
    }
    let length = le32(bytes, 16)?;
    if u64::from(length) > size.checked_sub(20)? {
        return None;
    }
    match bytes.get(12..16)? {
        b"VP8X" => {
            if length != 10 {
                return None;
            }
            Some((le24(bytes, 24)? + 1, le24(bytes, 27)? + 1))
        }
        b"VP8 " => {
            if length < 10
                || bytes.get(23..26) != Some(&[0x9d, 0x01, 0x2a][..])
                || *bytes.get(20)? & 1 != 0
            {
                return None;
            }
            let width = le16(bytes, 26)?;
            let height = le16(bytes, 28)?;
            if width & 0xc000 != 0 || height & 0xc000 != 0 {
                return None;
            }
            Some((u32::from(width), u32::from(height)))
        }
        b"VP8L" => {
            if length < 5 || *bytes.get(20)? != 0x2f {
                return None;
            }
            let bits = le32(bytes, 21)?;
            if bits >> 29 != 0 {
                return None;
            }
            Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        _ => None,
    }
}
#[cfg(test)]
mod tests;
