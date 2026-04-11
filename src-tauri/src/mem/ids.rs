//! Slug normalization for mem_item ids.
//! Format: `<kind>_<slug>` where slug is lowercase ASCII, words joined by `_`,
//! max 60 chars, deterministic from a title. Internal references use a
//! separate ULID `uid` column on mem_items so renames don't break relations.

use sha2::{Digest, Sha256};

const MAX_SLUG: usize = 60;

/// Normalize a free-form title to a slug body (no kind prefix).
/// If the title has no ASCII alphanumerics (e.g. pure Cyrillic / CJK),
/// falls back to a short hash of the title bytes so distinct non-ASCII
/// titles still get unique ids instead of all collapsing to `item`.
pub fn slugify(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut last_us = false;
    for c in title.chars() {
        let lc = c.to_ascii_lowercase();
        if lc.is_ascii_alphanumeric() {
            out.push(lc);
            last_us = false;
        } else if !last_us && !out.is_empty() {
            out.push('_');
            last_us = true;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() {
        let mut hasher = Sha256::new();
        hasher.update(title.as_bytes());
        let hash = hasher.finalize();
        out.push_str(&format!("u{}", &format!("{:x}", hash)[..8]));
    }
    if out.len() > MAX_SLUG {
        out.truncate(MAX_SLUG);
        while out.ends_with('_') {
            out.pop();
        }
    }
    out
}

/// Compose `kind_slug`. If a collision is detected by the caller, append
/// `_<short_hash>` of the seed (typically the body) for deterministic uniqueness.
pub fn compose(kind: &str, slug: &str) -> String {
    format!("{}_{}", kind, slug)
}

pub fn with_collision_suffix(kind: &str, slug: &str, seed: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    let hash = hasher.finalize();
    let short = format!("{:x}", hash)[..6].to_string();
    format!("{}_{}_{}", kind, slug, short)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Home Assistant"), "home_assistant");
        assert_eq!(slugify("API Timeout Fix!"), "api_timeout_fix");
        assert_eq!(slugify("  spaces  "), "spaces");
        assert!(slugify("---").starts_with('u'));
    }

    #[test]
    fn slugify_unicode_strips() {
        assert_eq!(slugify("Привет world"), "world");
    }

    #[test]
    fn slugify_pure_non_ascii_uses_hash_not_item() {
        let a = slugify("Андрей проставил очко");
        let b = slugify("Совершенно другая запись");
        assert!(a.starts_with('u') && a.len() == 9, "got {}", a);
        assert!(b.starts_with('u') && b.len() == 9, "got {}", b);
        assert_ne!(a, b, "different titles must produce different slugs");
        // Same title — same slug (deterministic).
        assert_eq!(a, slugify("Андрей проставил очко"));
    }

    #[test]
    fn slugify_truncates_at_60() {
        let long = "a".repeat(100);
        assert_eq!(slugify(&long).len(), 60);
    }

    #[test]
    fn compose_format() {
        assert_eq!(compose("skill", "home_assistant"), "skill_home_assistant");
    }

    #[test]
    fn collision_suffix_deterministic() {
        let a = with_collision_suffix("fact", "test", "body1");
        let b = with_collision_suffix("fact", "test", "body1");
        assert_eq!(a, b);
        let c = with_collision_suffix("fact", "test", "body2");
        assert_ne!(a, c);
    }
}
