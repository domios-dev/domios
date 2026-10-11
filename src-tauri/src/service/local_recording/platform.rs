#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SelectedInputEvent {
    Press {
        generation: u64,
    },
    Unavailable {
        generation: u64,
        reason: InputUnavailable,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum InputUnavailable {
    Removed,
    AccessLost,
    Error,
}

use codeterm_plugin::local_recording::{LocalRecordingTrigger, SelectedDevices};

pub(crate) fn supported() -> bool {
    cfg!(any(windows, target_os = "macos"))
}

pub(crate) use codeterm_plugin::local_recording::{
    LocalInputSetupError, LocalInputSubscription, SubscriptionTeardown,
};

pub(crate) trait LocalInputAdapter: Send + Sync {
    fn subscribe(
        &self,
        declaration: &LocalRecordingTrigger,
        selected: &SelectedDevices,
        generation: u64,
        callback: std::sync::Arc<dyn Fn(SelectedInputEvent) + Send + Sync>,
    ) -> Result<Box<dyn LocalInputSubscription>, LocalInputSetupError>;
}

pub(crate) struct UnsupportedInput;

impl LocalInputAdapter for UnsupportedInput {
    fn subscribe(
        &self,
        _: &LocalRecordingTrigger,
        _: &SelectedDevices,
        _: u64,
        _: std::sync::Arc<dyn Fn(SelectedInputEvent) + Send + Sync>,
    ) -> Result<Box<dyn LocalInputSubscription>, LocalInputSetupError> {
        Err(LocalInputSetupError {
            reason: "local_recording_unsupported".into(),
            teardown: None,
        })
    }
}

#[cfg(any(target_os = "macos", test))]
#[path = "platform/macos.rs"]
pub(crate) mod macos;
#[cfg(windows)]
#[path = "platform/windows.rs"]
pub(crate) mod windows;
