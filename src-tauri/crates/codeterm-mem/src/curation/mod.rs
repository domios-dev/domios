pub mod calibration;
pub mod merge;

pub fn validate_item_fields(
    title: &str,
    body: &str,
    area: &Option<String>,
    tags: &[String],
    importance: Option<f64>,
) -> Result<(), (u16, String)> {
    let t = title.trim();
    if t.is_empty() {
        return Err((400u16, "title must not be empty".into()));
    }
    if t.len() < 3 {
        return Err((400u16, "title must be >= 3 chars".into()));
    }
    if t.len() > 200 {
        return Err((400u16, "title must be <= 200 chars".into()));
    }
    if body.len() > 1_048_576 {
        return Err((
            400u16,
            format!("body too large: {} bytes (max 1 MB)", body.len()),
        ));
    }
    if body.trim().is_empty() {
        return Err((400u16, "body must not be empty".into()));
    }
    if let Some(a) = area.as_deref() {
        if !a.is_empty() {
            if a.len() > 64 {
                return Err((400u16, "area must be <= 64 chars".into()));
            }
            if !a
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                return Err((400u16, format!("area must match [a-zA-Z0-9_-]+: `{}`", a)));
            }
        }
    }
    if tags.len() > 20 {
        return Err((400u16, "too many tags (max 20)".into()));
    }
    for tag in tags {
        if tag.len() > 50 {
            return Err((400u16, format!("tag too long (max 50): `{}`", tag)));
        }
    }
    if let Some(i) = importance {
        if !(0.0..=5.0).contains(&i) {
            return Err((400u16, format!("importance must be 0.0..=5.0, got {}", i)));
        }
    }
    Ok(())
}

#[cfg(test)]
mod item_validation_tests {
    use super::validate_item_fields;

    #[test]
    fn save_rejects_nan_importance_and_empty_body() {
        assert!(validate_item_fields("valid title", "body", &None, &[], Some(f64::NAN)).is_err());
        assert!(validate_item_fields("valid title", "  \n", &None, &[], None).is_err());
    }
}

/// Reserved `changed_by` prefixes — only the server itself writes these.
/// Any client-supplied actor matching one of these is namespaced under
/// `user-overridden:` so it can never spoof a real worker / dreamer run
/// (which would trip the calibration re-entry guard or pollute the audit).
const RESERVED_CHANGED_BY_PREFIXES: &[&str] = &["worker:", "dreamer-", "dreaming:", "system:"];

pub fn sanitize_actor(actor: &str) -> String {
    if RESERVED_CHANGED_BY_PREFIXES
        .iter()
        .any(|p| actor.starts_with(p))
    {
        format!("user-overridden:{actor}")
    } else {
        actor.to_string()
    }
}

#[cfg(test)]
mod actor_sanitizer_tests {
    use super::sanitize_actor;

    #[test]
    fn plain_actor_passes_through() {
        assert_eq!(sanitize_actor("alice"), "alice");
        assert_eq!(sanitize_actor("cli:mem-pin"), "cli:mem-pin");
    }

    #[test]
    fn reserved_prefixes_are_wrapped() {
        assert_eq!(
            sanitize_actor("dreamer-calibration:promote"),
            "user-overridden:dreamer-calibration:promote",
        );
        assert_eq!(sanitize_actor("worker:rem"), "user-overridden:worker:rem");
        assert_eq!(
            sanitize_actor("dreaming:rem"),
            "user-overridden:dreaming:rem",
        );
        assert_eq!(
            sanitize_actor("system:dreaming"),
            "user-overridden:system:dreaming",
        );
    }
}

/// Choose the body to restore from an item's history. `version` targets a
/// specific history row id (its `new_body`, falling back to `old_body`);
/// `None` undoes the most recent body change (latest non-null `old_body`).
pub fn pick_restore_body(
    history: &[crate::store::HistoryEntry],
    version: Option<i64>,
) -> Result<String, (u16, String)> {
    match version {
        Some(v) => {
            let entry = history
                .iter()
                .find(|h| h.id == v)
                .ok_or((404u16, format!("history version {} not found", v)))?;
            entry
                .new_body
                .clone()
                .or_else(|| entry.old_body.clone())
                .ok_or((400u16, format!("history row {} has no body snapshot", v)))
        }
        None => history
            .iter()
            .rev()
            .find_map(|h| h.old_body.clone())
            .ok_or((400u16, "no prior body to restore".to_string())),
    }
}
