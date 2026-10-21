//! Messaging bridge, transcription, and outbound chat pipeline.

pub mod bridge;
pub mod chatpipeline;
pub mod ports;
pub mod registry;
pub mod services;
pub mod transcribe;

pub use bridge::{BridgeManager, BridgeRuntimeState, BridgeSupervisor};
pub use ports::{
    AttachmentStorePort, BridgeTabPort, ChatPreprocessorPort, ChatPreprocessorProvider,
    PreprocessCtx,
};
pub use registry::CapabilityRegistry;
pub use services::{find as find_service, ServiceDescriptor, SERVICES};
pub use transcribe::{
    TranscribeError, TranscribeRegistry, Transcriber, TranscriberStatus, Transcript,
};
