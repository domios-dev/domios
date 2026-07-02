//! Short, collision-checked entity id minting (typed base36 slugs).

const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

fn base36(mut n: u64) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let mut buf = Vec::new();
    while n > 0 {
        buf.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    buf.reverse();
    String::from_utf8(buf).expect("base36 digits are ascii")
}

fn slug(len: usize) -> String {
    let bytes = uuid::Uuid::new_v4().into_bytes();
    let n = u64::from_le_bytes(bytes[..8].try_into().expect("uuid has 16 bytes"));
    let modulus = 36u64.pow(len as u32);
    format!("{:0>width$}", base36(n % modulus), width = len)
}

/// Short id with a fresh salt and no store check (6 base36 chars). For standalone constructors.
pub fn short_id(prefix: &str) -> String {
    format!("{prefix}_{}", slug(6))
}

/// Collision-checked short id. Grows 6 -> 7 -> 8 chars, regenerating the salt on every collision.
pub fn mint_id(prefix: &str, taken: impl Fn(&str) -> bool) -> String {
    for len in [6usize, 7, 8] {
        for _ in 0..16 {
            let candidate = format!("{prefix}_{}", slug(len));
            if !taken(&candidate) {
                return candidate;
            }
        }
    }
    // Astronomically unlikely: a full uuid is unique by construction.
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn short_id_is_prefixed_and_bounded() {
        let id = short_id("task");
        let (prefix, slug) = id.split_once('_').expect("prefixed");
        assert_eq!(prefix, "task");
        assert_eq!(slug.len(), 6);
        assert!(slug.bytes().all(|b| b.is_ascii_alphanumeric() && !b.is_ascii_uppercase()));
    }

    #[test]
    fn mint_id_avoids_collisions_and_grows_on_saturation() {
        // Everything at length 6 is "taken" -> minter must escalate to a longer slug.
        let id = mint_id("task", |c| {
            c.split_once('_').map(|(_, s)| s.len() == 6).unwrap_or(false)
        });
        let slug = id.split_once('_').unwrap().1;
        assert!(slug.len() >= 7, "expected escalation past 6 chars, got {id}");
    }

    #[test]
    fn mint_id_returns_free_candidate() {
        let mut seen = HashSet::new();
        for _ in 0..2000 {
            let id = mint_id("sec", |c| seen.contains(c));
            assert!(seen.insert(id.clone()), "minted duplicate {id}");
        }
    }

    #[test]
    fn base36_roundtrips_small_values() {
        assert_eq!(base36(0), "0");
        assert_eq!(base36(35), "z");
        assert_eq!(base36(36), "10");
    }
}
