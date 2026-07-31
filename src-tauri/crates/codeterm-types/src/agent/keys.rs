//! Action-key → PTY byte translation, shared between the HTTP `send --actions`
//! handler and the generic state reactor so both inject keys identically.

/// Translate a single action-key name (`"down"`, `"enter"`, `"ctrl-c"`, …) into
/// the bytes a terminal expects. Returns `None` for an unknown key.
pub fn action_key_bytes(key: &str) -> Option<Vec<u8>> {
    if key.chars().count() == 1 {
        return Some(key.as_bytes().to_vec());
    }
    let normalized = key.replace('_', "-").to_ascii_lowercase();
    match normalized.as_str() {
        "enter" | "return" => Some(b"\r".to_vec()),
        "esc" | "escape" => Some(b"\x1b".to_vec()),
        "tab" => Some(b"\t".to_vec()),
        "backspace" | "bs" => Some(vec![0x7f]),
        "delete" | "del" => Some(b"\x1b[3~".to_vec()),
        "up" | "arrow-up" => Some(b"\x1b[A".to_vec()),
        "down" | "arrow-down" => Some(b"\x1b[B".to_vec()),
        "right" | "arrow-right" => Some(b"\x1b[C".to_vec()),
        "left" | "arrow-left" => Some(b"\x1b[D".to_vec()),
        "home" => Some(b"\x1b[H".to_vec()),
        "end" => Some(b"\x1b[F".to_vec()),
        "pageup" | "page-up" | "pgup" => Some(b"\x1b[5~".to_vec()),
        "pagedown" | "page-down" | "pgdn" => Some(b"\x1b[6~".to_vec()),
        "space" => Some(b" ".to_vec()),
        other => parse_ctrl_key(other).map(|b| vec![b]),
    }
}

fn parse_ctrl_key(value: &str) -> Option<u8> {
    let key = value
        .strip_prefix("ctrl-")
        .or_else(|| value.strip_prefix("control-"))
        .or_else(|| value.strip_prefix("c-"))?;
    if key.len() != 1 {
        return None;
    }
    let b = key.as_bytes()[0].to_ascii_lowercase();
    if b.is_ascii_lowercase() {
        Some(b - b'a' + 1)
    } else if b == b'[' {
        Some(0x1b)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrow_and_enter_map_to_escape_sequences() {
        assert_eq!(action_key_bytes("down"), Some(b"\x1b[B".to_vec()));
        assert_eq!(action_key_bytes("enter"), Some(b"\r".to_vec()));
    }

    #[test]
    fn underscore_and_case_normalized() {
        assert_eq!(action_key_bytes("ARROW_DOWN"), Some(b"\x1b[B".to_vec()));
    }

    #[test]
    fn ctrl_key_maps_to_control_byte() {
        assert_eq!(action_key_bytes("ctrl-c"), Some(vec![3]));
    }

    #[test]
    fn literal_character_maps_to_utf8_bytes() {
        assert_eq!(action_key_bytes("y"), Some(b"y".to_vec()));
        assert_eq!(action_key_bytes("A"), Some(b"A".to_vec()));
        assert_eq!(action_key_bytes("✓"), Some("✓".as_bytes().to_vec()));
    }

    #[test]
    fn unknown_key_is_none() {
        assert_eq!(action_key_bytes("frobnicate"), None);
    }
}
