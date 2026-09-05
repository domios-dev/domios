//! The running executable's debug identity, for the crash sidecar.
//!
//! A dump is only symbolizable against the exact symbol file that was linked with
//! the binary. Recording the identity turns "this dump is from some build" into a
//! lookup key: on Windows the RSDS GUID + age that names `codeterm.pdb`, on Linux
//! the GNU build-id, on macOS the Mach-O UUID.
//!
//! Resolved once at startup and cached — the crash handler only formats a string
//! it already has, never touches the filesystem.

use std::io::{Read, Seek, SeekFrom};
use std::sync::OnceLock;

pub const IDENTITY_UNAVAILABLE: &str = "<unavailable>";

/// What the sidecar prints, and the key the symbols are filed under. The key is
/// the canonical Windows symbol-store form so `build.sh` and
/// `scripts/pdb_identity.py` can compute the same string from the PDB alone.
pub struct Identity {
    pub display: String,
    pub key: String,
}

fn identity() -> Option<&'static Identity> {
    static ID: OnceLock<Option<Identity>> = OnceLock::new();
    ID.get_or_init(|| std::env::current_exe().ok().and_then(|p| extract(&p)))
        .as_ref()
}

/// `Debug id:` in the sidecar. Cached; safe to call from a crash handler.
pub fn image_debug_id() -> &'static str {
    identity().map_or(IDENTITY_UNAVAILABLE, |i| i.display.as_str())
}

/// The directory name this build's symbols are retained under.
pub fn debug_key() -> Option<&'static str> {
    identity().map(|i| i.key.as_str())
}

/// `Image:` in the sidecar — where the binary that crashed lived.
pub fn image_path() -> &'static str {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| {
        std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| IDENTITY_UNAVAILABLE.to_string())
    })
}

/// Render an RSDS CodeView record as `GUID age=N pdb=NAME` — the same shape
/// `scripts/symbolicate-minidump.py` prints for a dump's module, so the two can be
/// compared by eye. The GUID's first three fields are little-endian, the rest is
/// byte order as stored; that is the layout every Windows symbol tool expects.
#[cfg(test)]
pub fn render_rsds(record: &[u8]) -> Option<String> {
    rsds(record).map(|i| i.display)
}

/// The symbol-store key for an RSDS record: 32 GUID hex digits then the age in hex.
#[cfg(test)]
pub fn rsds_key(record: &[u8]) -> Option<String> {
    rsds(record).map(|i| i.key)
}

#[cfg(any(windows, test))]
fn rsds(record: &[u8]) -> Option<Identity> {
    if record.len() < 24 || &record[0..4] != b"RSDS" {
        return None;
    }
    let d1 = u32::from_le_bytes(record[4..8].try_into().ok()?);
    let d2 = u16::from_le_bytes(record[8..10].try_into().ok()?);
    let d3 = u16::from_le_bytes(record[10..12].try_into().ok()?);
    let d4 = &record[12..20];
    let age = u32::from_le_bytes(record[20..24].try_into().ok()?);

    let tail: String = d4.iter().map(|b| format!("{:02X}", b)).collect();
    let name = record[24..]
        .split(|b| *b == 0)
        .next()
        .map(|n| String::from_utf8_lossy(n).into_owned())
        .unwrap_or_default();
    let name = name.replace('\\', "/");
    let name = name.rsplit('/').next().unwrap_or("").to_string();

    Some(Identity {
        display: format!(
            "{:08X}-{:04X}-{:04X}-{}-{} age={} pdb={}",
            d1,
            d2,
            d3,
            &tail[0..4],
            &tail[4..],
            age,
            name
        ),
        key: format!("{:08X}{:04X}{:04X}{}{:X}", d1, d2, d3, tail, age),
    })
}

/// Render a raw build-id / UUID byte string as uppercase hex.
#[cfg(any(target_os = "linux", target_os = "macos", test))]
pub fn render_hex_id(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02X}", b)).collect()
}

fn read_at(file: &mut std::fs::File, offset: u64, len: usize) -> Option<Vec<u8>> {
    if len == 0 || len > 1 << 20 {
        return None;
    }
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut buf = vec![0u8; len];
    file.read_exact(&mut buf).ok()?;
    Some(buf)
}

fn u16_at(buf: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(buf.get(off..off + 2)?.try_into().ok()?))
}

fn u32_at(buf: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(buf.get(off..off + 4)?.try_into().ok()?))
}

#[cfg(target_os = "windows")]
fn extract(path: &std::path::Path) -> Option<Identity> {
    let mut file = std::fs::File::open(path).ok()?;
    let head = read_at(&mut file, 0, 0x40)?;
    let pe_off = u32_at(&head, 0x3C)? as u64;

    let coff = read_at(&mut file, pe_off, 24)?;
    let sections = u16_at(&coff, 6)?;
    let opt_size = u16_at(&coff, 20)?;
    let opt_off = pe_off + 24;
    let opt = read_at(&mut file, opt_off, opt_size as usize)?;
    // IMAGE_DIRECTORY_ENTRY_DEBUG is the 7th data directory.
    let dir_base = if u16_at(&opt, 0)? == 0x20B { 112 } else { 96 };
    let debug_rva = u32_at(&opt, dir_base + 6 * 8)?;
    let debug_size = u32_at(&opt, dir_base + 6 * 8 + 4)?;
    if debug_rva == 0 || debug_size == 0 {
        return None;
    }

    let table = read_at(&mut file, opt_off + opt_size as u64, sections as usize * 40)?;
    let to_offset = |rva: u32| -> Option<u64> {
        (0..sections as usize).find_map(|i| {
            let s = i * 40;
            let virt_size = u32_at(&table, s + 8)?;
            let virt_addr = u32_at(&table, s + 12)?;
            let raw_size = u32_at(&table, s + 16)?;
            let raw_ptr = u32_at(&table, s + 20)?;
            let span = virt_size.max(raw_size);
            (rva >= virt_addr && rva < virt_addr.saturating_add(span))
                .then(|| (raw_ptr + (rva - virt_addr)) as u64)
        })
    };

    let entries = read_at(&mut file, to_offset(debug_rva)?, debug_size as usize)?;
    for i in 0..(debug_size as usize / 28) {
        let e = i * 28;
        if u32_at(&entries, e + 12)? != 2 {
            continue; // IMAGE_DEBUG_TYPE_CODEVIEW
        }
        let size = u32_at(&entries, e + 16)? as usize;
        let ptr = u32_at(&entries, e + 24)? as u64;
        let record = read_at(&mut file, ptr, size)?;
        return rsds(&record);
    }
    None
}

#[cfg(target_os = "linux")]
fn extract(path: &std::path::Path) -> Option<Identity> {
    let mut file = std::fs::File::open(path).ok()?;
    let head = read_at(&mut file, 0, 64)?;
    if &head[0..4] != b"\x7fELF" || head[4] != 2 {
        return None;
    }
    let ph_off = u64::from_le_bytes(head.get(32..40)?.try_into().ok()?);
    let ph_size = u16_at(&head, 54)? as usize;
    let ph_count = u16_at(&head, 56)? as usize;

    for i in 0..ph_count {
        let ph = read_at(&mut file, ph_off + (i * ph_size) as u64, ph_size)?;
        if u32_at(&ph, 0)? != 4 {
            continue; // PT_NOTE
        }
        let offset = u64::from_le_bytes(ph.get(8..16)?.try_into().ok()?);
        let size = u64::from_le_bytes(ph.get(32..40)?.try_into().ok()?) as usize;
        let notes = read_at(&mut file, offset, size)?;
        let mut cursor = 0usize;
        while cursor + 12 <= notes.len() {
            let name_size = u32_at(&notes, cursor)? as usize;
            let desc_size = u32_at(&notes, cursor + 4)? as usize;
            let note_type = u32_at(&notes, cursor + 8)?;
            let name_at = cursor + 12;
            let desc_at = name_at + name_size.div_ceil(4) * 4;
            if note_type == 3 && notes.get(name_at..name_at + 4) == Some(b"GNU\0") {
                let id = notes.get(desc_at..desc_at + desc_size)?;
                let hex = render_hex_id(id);
                return Some(Identity {
                    display: format!("build-id={}", hex),
                    key: hex,
                });
            }
            cursor = desc_at + desc_size.div_ceil(4) * 4;
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn extract(path: &std::path::Path) -> Option<Identity> {
    let mut file = std::fs::File::open(path).ok()?;
    let head = read_at(&mut file, 0, 32)?;
    let magic = u32_at(&head, 0)?;
    if magic != 0xFEED_FACF {
        return None; // 64-bit Mach-O only; a fat binary is opened by slice elsewhere
    }
    let commands = u32_at(&head, 16)?;
    let mut cursor = 32u64;
    for _ in 0..commands {
        let header = read_at(&mut file, cursor, 8)?;
        let kind = u32_at(&header, 0)?;
        let size = u32_at(&header, 4)? as u64;
        if size < 8 {
            return None;
        }
        if kind == 0x1B {
            // LC_UUID
            let uuid = read_at(&mut file, cursor + 8, 16)?;
            let hex = render_hex_id(&uuid);
            return Some(Identity {
                display: format!("uuid={}", hex),
                key: hex,
            });
        }
        cursor += size;
    }
    None
}

#[cfg(test)]
mod tests;
