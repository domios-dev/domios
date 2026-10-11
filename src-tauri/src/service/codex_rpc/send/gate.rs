use super::super::lifecycle::DeliveryState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SendGate {
    Pty,
    Refuse(&'static str),
    Rpc,
}

impl SendGate {
    pub(super) fn classify(
        capability: Option<bool>,
        state: DeliveryState,
        endpoint_available: bool,
        binding_verified: bool,
        first_readiness_reached: bool,
    ) -> Self {
        if capability == Some(false) || state == DeliveryState::Plain {
            return Self::Pty;
        }
        if state == DeliveryState::Pending && !first_readiness_reached {
            return Self::Refuse(
                "programmatic send before first readiness; user sends require durable admission",
            );
        }
        if !endpoint_available {
            return Self::Refuse("sidecar unavailable; lifecycle recovery pending");
        }
        if capability != Some(true) || !binding_verified {
            return Self::Refuse("sidecar thread binding pending");
        }
        Self::Rpc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_rpc_send_before_readiness_requires_durable_admission() {
        assert!(matches!(
            SendGate::classify(Some(true), DeliveryState::Pending, true, true, false),
            SendGate::Refuse(_)
        ));
    }

    #[test]
    fn codex_rpc_send_post_ready_attachment_loss_does_not_gate_verified_session_input() {
        assert_eq!(
            SendGate::classify(Some(true), DeliveryState::Pending, true, true, true),
            SendGate::Rpc
        );
    }

    #[test]
    fn codex_rpc_send_requires_available_endpoint_and_own_binding() {
        for state in [DeliveryState::Pending, DeliveryState::Ready] {
            for ready in [false, true] {
                for (endpoint, binding) in [(true, false), (false, true), (false, false)] {
                    assert!(matches!(
                        SendGate::classify(Some(true), state, endpoint, binding, ready),
                        SendGate::Refuse(_)
                    ));
                }
            }
        }
        assert_eq!(
            SendGate::classify(Some(true), DeliveryState::Ready, true, true, false),
            SendGate::Rpc
        );
        assert!(matches!(
            SendGate::classify(None, DeliveryState::Ready, true, true, true),
            SendGate::Refuse(_)
        ));
    }

    #[test]
    fn codex_rpc_send_plain_recovery_and_capability_off_restore_pty() {
        for capability in [None, Some(true), Some(false)] {
            assert_eq!(
                SendGate::classify(capability, DeliveryState::Plain, false, false, false),
                SendGate::Pty
            );
        }
        for state in [
            DeliveryState::Pending,
            DeliveryState::Ready,
            DeliveryState::Plain,
        ] {
            assert_eq!(
                SendGate::classify(Some(false), state, true, true, false),
                SendGate::Pty
            );
        }
    }
}
