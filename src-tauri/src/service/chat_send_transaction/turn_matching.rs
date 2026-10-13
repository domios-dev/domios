pub(crate) fn normalize_turn_evidence_with(
    text: &str,
    normalization: crate::agent::provider::TurnEvidenceNormalization,
) -> String {
    normalization.normalize(&crate::service::pty_io::sanitize_agent_prompt_input(text))
}

pub(crate) fn normalize_written_payload(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string()
}

pub(crate) fn contains_written_payload(record: &str, payload: &str) -> bool {
    let payload = normalize_written_payload(payload);
    !payload.is_empty() && normalize_written_payload(record).contains(&payload)
}

pub(super) fn matching_turn_send_ids(
    turn_text: &str,
    candidates: &[(
        String,
        String,
        crate::agent::provider::TurnEvidenceNormalization,
    )],
) -> Vec<String> {
    let evidence = normalize_written_payload(turn_text);
    let mut offset = 0;
    let mut matched = Vec::new();
    for (send_id, expected, _) in candidates {
        let expected = normalize_written_payload(expected);
        if expected.is_empty() {
            continue;
        }
        if let Some(start) = evidence[offset..].find(&expected) {
            offset += start + expected.len();
            matched.push(send_id.clone());
        }
    }
    matched
}

#[cfg(test)]
mod tests;
