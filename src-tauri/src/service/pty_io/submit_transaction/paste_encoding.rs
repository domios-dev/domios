use crate::agent::provider::{MultilinePasteStrategy, PasteCapability, VerifySubmitStrategy};

pub(super) const BRACKETED_PASTE_START: &str = "\x1b[200~";
pub(super) const BRACKETED_PASTE_END: &str = "\x1b[201~";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PasteEncodingCapability {
    Unsupported,
    TaggedBlock,
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
            match paste.and_then(|paste| paste.multiline) {
                Some(MultilinePasteStrategy::TaggedBlock) => Self::TaggedBlock,
                None => Self::Unsupported,
            }
        } else {
            Self::for_strategy(strategy)
        }
    }

    fn bracketed(self, text: &str, live_mode: Option<bool>) -> bool {
        !matches!(self, Self::Unsupported | Self::TaggedBlock)
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
    if capability == PasteEncodingCapability::TaggedBlock {
        let message = format!(
            "{}{}{}",
            typed_prefix.unwrap_or_default(),
            text,
            typed_suffix.unwrap_or_default()
        );
        let normalized = message.replace("\r\n", "\n").replace('\r', "\n");
        if normalized.contains('\n') {
            let mut tag = "domios".to_string();
            let mut suffix = 0;
            while normalized.contains(&format!("{tag}}}")) {
                suffix += 1;
                tag = format!("domios{suffix}");
            }
            return format!("{{{tag}\r{}\r{tag}}}", normalized.replace('\n', "\r")).into_bytes();
        }
        return normalized.into_bytes();
    }
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

pub(super) fn resolve_pty_paste_capability(
    mux: &crate::multiplexer::commands::MuxState,
    registry: &crate::agent::registry::AgentRegistry,
    pty_id: &str,
    strategy: &VerifySubmitStrategy,
) -> (Option<String>, PasteEncodingCapability) {
    let provider = {
        let mux = mux.read();
        mux.tab_id_for_pty(pty_id)
            .and_then(|tab| mux.get_tab(&tab).and_then(|tab| tab.agent_type.clone()))
    };
    let declared = provider.as_ref().and_then(|provider| {
        registry
            .with_provider(provider, |provider| provider.paste_capability())
            .flatten()
    });
    let capability = PasteEncodingCapability::for_provider(strategy, declared.as_ref());
    (provider, capability)
}
