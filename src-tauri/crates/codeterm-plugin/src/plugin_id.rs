pub fn valid(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 128
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        return false;
    }
    !matches!(value, "con" | "prn" | "aux" | "nul")
        && !["com", "lpt"].iter().any(|prefix| {
            value.strip_prefix(prefix).is_some_and(|suffix| {
                suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9')
            })
        })
}

pub fn validate(value: &str) -> Result<(), String> {
    valid(value)
        .then_some(())
        .ok_or_else(|| "invalid confined plugin identity".into())
}

pub fn validate_runtime(id: &str, manifest: Option<&str>) -> Result<(), String> {
    validate(id)?;
    if let Some(manifest) = manifest {
        let manifest: serde_json::Value =
            serde_json::from_str(manifest).map_err(|_| "invalid runtime manifest")?;
        if manifest
            .get("id")
            .is_some_and(|value| value.as_str() != Some(id))
        {
            return Err("manifest runtime identity mismatch".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
