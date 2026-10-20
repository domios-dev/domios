pub(crate) fn normalize_turn_evidence_with(
    text: &str,
    normalization: crate::agent::provider::TurnEvidenceNormalization,
) -> String {
    normalization.normalize(&codeterm_pty::pty::ansi::sanitize_agent_prompt_input(text))
}

pub(crate) fn normalize_written_payload(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string()
}

pub(crate) fn contains_written_payload(record: &str, payload: &str) -> bool {
    written_payload_match_end(record, payload, Default::default(), 0).is_some()
}

pub(crate) fn written_payload_match_end(
    record: &str,
    payload: &str,
    normalization: crate::agent::provider::TurnEvidenceNormalization,
    offset: usize,
) -> Option<usize> {
    let canonical_record = normalize_written_payload(record);
    let canonical_payload = normalize_written_payload(payload);
    let (record, payload) = if normalization == Default::default() {
        (canonical_record, canonical_payload)
    } else {
        (
            normalization.normalize(&canonical_record),
            normalization.normalize(&canonical_payload),
        )
    };
    if payload.is_empty() {
        return None;
    }
    record
        .get(offset..)?
        .find(&payload)
        .map(|start| offset + start + payload.len())
}

pub(super) fn matching_turn_send_ids(
    turn_text: &str,
    candidates: &[(
        String,
        String,
        crate::agent::provider::TurnEvidenceNormalization,
    )],
) -> Vec<String> {
    let mut offset = 0;
    let mut scope = None;
    let mut matched = Vec::new();
    for (send_id, expected, normalization) in candidates {
        if scope.is_some_and(|scope| scope != *normalization) {
            continue;
        }
        if let Some(end) = written_payload_match_end(turn_text, expected, *normalization, offset) {
            offset = end;
            scope = Some(*normalization);
            matched.push(send_id.clone());
        }
    }
    matched
}

#[cfg(test)]
mod tests;
