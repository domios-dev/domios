use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

fn skip_blocks(file: &mut File) -> Option<()> {
    loop {
        if file.stream_position().ok()? > 256 * 1024 {
            return None;
        }
        let mut length = [0];
        file.read_exact(&mut length).ok()?;
        if length[0] == 0 {
            return Some(());
        }
        file.seek(SeekFrom::Current(i64::from(length[0]))).ok()?;
    }
}

pub(super) fn inspect(path: &Path, extension: &str) -> Option<((u32, u32), bool)> {
    let mut file = File::open(path).ok()?;
    let mut head = [0u8; 30];
    let count = file.read(&mut head).ok()?;
    if count < 13 {
        return None;
    }
    if extension == "gif" && matches!(&head[..6], b"GIF87a" | b"GIF89a") {
        let dimensions = (
            u32::from(u16::from_le_bytes([head[6], head[7]])),
            u32::from(u16::from_le_bytes([head[8], head[9]])),
        );
        let global = if head[10] & 0x80 != 0 {
            3 * (1u64 << ((head[10] & 7) + 1))
        } else {
            0
        };
        file.seek(SeekFrom::Start(13 + global)).ok()?;
        let mut frames = 0;
        loop {
            if file.stream_position().ok()? > 256 * 1024 {
                return Some((dimensions, true));
            }
            let mut tag = [0];
            file.read_exact(&mut tag).ok()?;
            match tag[0] {
                0x21 => {
                    file.read_exact(&mut tag).ok()?;
                    if skip_blocks(&mut file).is_none() {
                        return Some((dimensions, true));
                    }
                }
                0x2c => {
                    frames += 1;
                    if frames > 1 {
                        return Some((dimensions, true));
                    }
                    let mut descriptor = [0; 9];
                    file.read_exact(&mut descriptor).ok()?;
                    if descriptor[8] & 0x80 != 0 {
                        file.seek(SeekFrom::Current(3 * (1i64 << ((descriptor[8] & 7) + 1))))
                            .ok()?;
                    }
                    file.read_exact(&mut tag).ok()?;
                    if skip_blocks(&mut file).is_none() {
                        return Some((dimensions, true));
                    }
                }
                0x3b => return Some((dimensions, false)),
                _ => return None,
            }
        }
    }
    if extension != "webp" || &head[..4] != b"RIFF" || &head[8..12] != b"WEBP" {
        return None;
    }
    let le24 = |index| u32::from_le_bytes([head[index], head[index + 1], head[index + 2], 0]) + 1;
    match &head[12..16] {
        b"VP8X" if count >= 30 => Some(((le24(24), le24(27)), head[20] & 2 != 0)),
        b"VP8 " if count >= 30 && head[23..26] == [0x9d, 0x01, 0x2a] => Some((
            (
                u32::from(u16::from_le_bytes([head[26], head[27]]) & 0x3fff),
                u32::from(u16::from_le_bytes([head[28], head[29]]) & 0x3fff),
            ),
            false,
        )),
        b"VP8L" if count >= 25 && head[20] == 0x2f => {
            let bits = u32::from_le_bytes([head[21], head[22], head[23], head[24]]);
            Some((((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1), false))
        }
        _ => None,
    }
}
