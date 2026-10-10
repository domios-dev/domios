use super::super::lifecycle::DeliveryState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SendGate {
    Pty,
    Hold(&'static str),
    Rpc,
}

impl SendGate {
    pub(super) fn classify(
        capability: Option<bool>,
        state: DeliveryState,
        endpoint_available: bool,
        binding_verified: bool,
    ) -> Self {
        if capability == Some(false) {
            return Self::Pty;
        }
        match state {
            DeliveryState::Plain => Self::Pty,
            DeliveryState::Pending => Self::Hold("remote attachment or launch consumption pending"),
            DeliveryState::Ready if !endpoint_available => {
                Self::Hold("sidecar unavailable; lifecycle recovery pending")
            }
            DeliveryState::Ready if capability != Some(true) || !binding_verified => {
                Self::Hold("sidecar thread binding pending")
            }
            DeliveryState::Ready => Self::Rpc,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_rpc_send_pending_attach_holds_even_with_verified_server_binding() {
        assert!(matches!(
            SendGate::classify(Some(true), DeliveryState::Pending, true, true),
            SendGate::Hold(_)
        ));
    }

    #[test]
    fn codex_rpc_send_failed_attach_holds_after_endpoint_and_identity_clear() {
        for state in [DeliveryState::Pending, DeliveryState::Ready] {
            for capability in [None, Some(true)] {
                assert!(matches!(
                    SendGate::classify(capability, state, false, false),
                    SendGate::Hold(_)
                ));
            }
        }
    }

    #[test]
    fn codex_rpc_send_ready_requires_available_endpoint_and_own_binding() {
        for (endpoint, binding) in [(true, false), (false, true), (false, false)] {
            assert!(matches!(
                SendGate::classify(Some(true), DeliveryState::Ready, endpoint, binding),
                SendGate::Hold(_)
            ));
        }
        assert_eq!(
            SendGate::classify(Some(true), DeliveryState::Ready, true, true),
            SendGate::Rpc
        );
    }

    #[test]
    fn codex_rpc_send_plain_recovery_and_capability_off_restore_pty() {
        for capability in [None, Some(true), Some(false)] {
            assert_eq!(
                SendGate::classify(capability, DeliveryState::Plain, false, false),
                SendGate::Pty
            );
        }
        for state in [
            DeliveryState::Pending,
            DeliveryState::Ready,
            DeliveryState::Plain,
        ] {
            assert_eq!(
                SendGate::classify(Some(false), state, true, true),
                SendGate::Pty
            );
        }
    }
}
