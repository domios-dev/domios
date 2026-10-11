use crate::agent::provider::{PasteCapability, VerifySubmitStrategy};

pub(super) const BRACKETED_PASTE_START: &str = "\x1b[200~";
pub(super) const BRACKETED_PASTE_END: &str = "\x1b[201~";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PasteEncodingCapability {
    Unsupported,
    LiveModeOnly,
    MultilineTui,
}

impl PasteEncodingCapability {
    pub(super) fn for_strategy(strategy: &VerifySubmitStrategy) -> Self {
        if matches!(strategy, VerifySubmitStrategy::TuiInputDrained { .. }) {
            Self::MultilineTui
        } else {
            Self::LiveModeOnly
        }
    }

    pub(super) fn for_provider(
        strategy: &VerifySubmitStrategy,
        paste: Option<&PasteCapability>,
    ) -> Self {
        if paste.is_some_and(|paste| !paste.supports_bracketed_paste) {
            Self::Unsupported
        } else {
            Self::for_strategy(strategy)
        }
    }

    fn bracketed(self, text: &str, live_mode: Option<bool>) -> bool {
        self != Self::Unsupported
            && live_mode.unwrap_or_else(|| {
                self == Self::MultilineTui && (text.contains('\r') || text.contains('\n'))
            })
    }
}

pub(super) fn xterm_paste(
    text: &str,
    live_mode: Option<bool>,
    capability: PasteEncodingCapability,
) -> String {
    if capability.bracketed(text, live_mode) {
        let normalized = text.replace("\r\n", "\n");
        format!("{BRACKETED_PASTE_START}{normalized}{BRACKETED_PASTE_END}")
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r")
    }
}

pub(crate) fn live_command_input(text: &str, live_mode: Option<bool>) -> Vec<u8> {
    let mut input =
        xterm_paste(text, live_mode, PasteEncodingCapability::LiveModeOnly).into_bytes();
    input.push(b'\r');
    input
}

pub(super) fn encode_paste_frame(
    text: &str,
    typed_prefix: Option<&str>,
    typed_suffix: Option<&str>,
    live_mode: Option<bool>,
    capability: PasteEncodingCapability,
) -> Vec<u8> {
    let paste = xterm_paste(
        text,
        typed_prefix.is_some().then_some(true).or(live_mode),
        capability,
    );
    let mut input = typed_prefix.unwrap_or_default().as_bytes().to_vec();
    input.extend_from_slice(paste.as_bytes());
    input.extend_from_slice(typed_suffix.unwrap_or_default().as_bytes());
    input
}
