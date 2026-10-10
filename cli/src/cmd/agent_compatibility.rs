pub(super) fn status_line(provider: &serde_json::Value) -> Option<String> {
    let status = provider.get("cli_status")?;
    if status.get("required").and_then(serde_json::Value::as_bool) == Some(false) {
        return None;
    }
    let version = status
        .get("detected_version")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let verdict = status
        .pointer("/compatibility/verdict")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let probe = status
        .get("version_probe")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    Some(format!(
        "CLI version={version} compatibility={verdict} probe={probe}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_compat_cli_preserves_unknown_and_pending() {
        let provider = serde_json::json!({"cli_status": {"required": true, "detected_version": null, "version_probe": "pending", "compatibility": {"verdict": "unknown"}}});
        let rendered = status_line(&provider).unwrap();
        assert!(rendered.contains("version=unknown"));
        assert!(rendered.contains("compatibility=unknown"));
        assert!(rendered.contains("probe=pending"));
        assert_eq!(
            status_line(&serde_json::json!({"cli_status": {"required": false}})),
            None
        );
    }
}
