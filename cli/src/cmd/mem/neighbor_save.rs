pub(super) fn payload(
    kind: &str,
    area: &str,
    title: &str,
    body: String,
    summary: &Option<String>,
    tags: &[String],
) -> serde_json::Value {
    serde_json::json!({"kind":kind,"area":area,"title":title,"body":body,"summary":summary,"tags":tags})
}

#[cfg(test)]
mod tests;
