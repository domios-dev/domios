pub(super) fn review_verdict(value: &serde_json::Value) -> Result<&str, String> {
    let object = value
        .as_object()
        .ok_or("agent review must return a typed verdict")?;
    let verdict = object
        .get("verdict")
        .and_then(serde_json::Value::as_str)
        .filter(|value| matches!(*value, "pass" | "retry" | "choose"))
        .ok_or("agent review verdict unavailable")?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "verdict" | "notes" | "choice"))
        || object.get("notes").is_some_and(|value| {
            value
                .as_str()
                .is_none_or(|text| text.len() > 4096 || text.contains('\0'))
        })
        || object
            .get("choice")
            .is_some_and(|value| value.as_u64().is_none_or(|index| index >= 256))
        || (verdict == "choose" && !object.contains_key("choice"))
        || (verdict != "choose" && object.contains_key("choice"))
    {
        return Err("invalid bounded agent review verdict".into());
    }
    Ok(verdict)
}

#[cfg(test)]
mod tests;
