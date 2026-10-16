//! Producer scalar predicates (`scalarText`, `validId`, `seconds`, installation id).

pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
pub const MAX_SECONDS: u64 = 253_402_300_799;

/// Non-empty, at most `max` Unicode scalars, no C0 controls or DEL.
pub fn scalar_text(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.chars().count() <= max
        && !value.chars().any(|c| (c as u32) < 0x20 || c == '\u{7f}')
}

/// Producer `validId`: length counts UTF-16 code units.
pub fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.encode_utf16().count() <= 128
        && !value.chars().any(|c| c == '/' || (c as u32) < 0x20)
        && value != "."
        && value != ".."
}

pub fn valid_installation_id(value: &str) -> bool {
    scalar_text(value, 128) && !value.contains(['/', '\\']) && value != "." && value != ".."
}

pub fn seconds(value: u64) -> bool {
    value <= MAX_SECONDS
}

pub fn safe_integer(value: u64) -> bool {
    value <= MAX_SAFE_INTEGER
}
