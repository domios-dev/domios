//! App-side adapter — wires plugin capability registration to bridge-owned registries.

pub use codeterm_bridge::transcribe::{
    TranscribeError, TranscribeRegistry, Transcriber, TranscriberStatus, Transcript,
};
