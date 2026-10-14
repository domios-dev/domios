#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PairingSendDisposition {
    DefinitelyBeforeSend,
    Uncertain,
    Terminal,
}

pub(super) fn classify_reqwest_error(
    is_timeout: bool,
    is_connect: bool,
    is_response_phase: bool,
) -> PairingSendDisposition {
    if is_timeout || is_response_phase {
        PairingSendDisposition::Uncertain
    } else if is_connect {
        PairingSendDisposition::DefinitelyBeforeSend
    } else {
        PairingSendDisposition::Uncertain
    }
}

pub(super) fn safe_reqwest_detail(error: &reqwest::Error) -> String {
    let disposition = classify_reqwest_error(
        error.is_timeout(),
        error.is_connect(),
        error.is_body() || error.is_decode(),
    );
    let phase = match disposition {
        PairingSendDisposition::DefinitelyBeforeSend => "connection_establishment",
        PairingSendDisposition::Uncertain if error.is_timeout() => "timeout",
        PairingSendDisposition::Uncertain if error.is_body() => "response_body",
        PairingSendDisposition::Uncertain if error.is_decode() => "response_decode",
        PairingSendDisposition::Uncertain => "unknown",
        PairingSendDisposition::Terminal => "terminal",
    };
    crate::mesh::transport::safe_reqwest_error_detail(error, phase)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_connect_peer_timeout_evidence_stays_uncertain_when_connect_overlaps() {
        assert_eq!(
            classify_reqwest_error(true, true, false),
            PairingSendDisposition::Uncertain
        );
        assert_eq!(
            classify_reqwest_error(false, true, true),
            PairingSendDisposition::Uncertain
        );
    }

    #[test]
    fn mesh_connect_peer_flattened_connect_label_is_not_before_send_evidence() {
        assert_eq!(
            classify_reqwest_error(false, false, false),
            PairingSendDisposition::Uncertain
        );
    }
}
