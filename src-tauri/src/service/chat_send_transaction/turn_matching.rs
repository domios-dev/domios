/// The shape a delivered payload takes when it reappears as a session user
/// turn: control bytes stripped by the send path, surrounding whitespace
/// trimmed by TUI/transcript rendering.
pub(crate) fn normalize_turn_evidence_with(
    text: &str,
    normalization: crate::agent::provider::TurnEvidenceNormalization,
) -> String {
    normalization.normalize(&crate::service::pty_io::sanitize_agent_prompt_input(text))
}

pub(super) fn matching_turn_send_ids(
    turn_text: &str,
    candidates: &[(
        String,
        String,
        crate::agent::provider::TurnEvidenceNormalization,
    )],
) -> Vec<String> {
    for start in 0..candidates.len() {
        let normalization = candidates[start].2;
        let evidence = normalize_turn_evidence_with(turn_text, normalization);
        let mut matched = Vec::new();
        let mut offset = 0;
        for (send_id, expected, candidate_normalization) in candidates.iter().skip(start) {
            if *candidate_normalization != normalization || expected.is_empty() {
                break;
            }
            if !matched.is_empty() {
                while let Some(character) = evidence
                    .get(offset..)
                    .and_then(|remaining| remaining.chars().next())
                {
                    if !character.is_whitespace() {
                        break;
                    }
                    offset += character.len_utf8();
                }
            }
            if !evidence
                .get(offset..)
                .is_some_and(|remaining| remaining.starts_with(expected.as_str()))
            {
                break;
            }
            offset += expected.len();
            matched.push(send_id.clone());
            if matched.len() >= 2 && evidence.get(offset..).is_some_and(str::is_empty) {
                return matched;
            }
        }
    }
    for (send_id, expected, normalization) in candidates {
        let evidence = normalize_turn_evidence_with(turn_text, *normalization);
        if !evidence.is_empty() && evidence.as_str() == expected.as_str() {
            return vec![send_id.clone()];
        }
    }
    Vec::new()
}
