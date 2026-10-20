use codeterm_local_recording::LocalRecordingTrigger;
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectedInputEvent {
    Press {
        generation: u64,
    },
    Unavailable {
        generation: u64,
        reason: InputUnavailable,
    },
}

#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputUnavailable {
    Removed,
    AccessLost,
    Error,
}

use codeterm_plugin::local_recording::SelectedDevices;
use std::sync::Arc;

pub use codeterm_plugin::local_recording::RecordingInputDevice;

pub fn input_devices(trigger: &LocalRecordingTrigger) -> Result<Vec<RecordingInputDevice>, String> {
    #[cfg(windows)]
    {
        if !windows::supports_trigger(trigger) {
            return Ok(Vec::new());
        }
        return windows::selectable_inputs(trigger).map(|inputs| {
            inputs
                .into_iter()
                .map(|input| RecordingInputDevice {
                    id: input.input_id.clone(),
                    label: input.input_id.clone(),
                    kind: codeterm_plugin::local_recording::RecordingInputKind::Hid,
                    connected: true,
                    input_id: input.input_id,
                    collection_id: input.collection_id,
                })
                .collect()
        });
    }
    #[cfg(not(windows))]
    {
        let _ = trigger;
        Ok(Vec::new())
    }
}

pub fn supported() -> bool {
    cfg!(any(windows, target_os = "macos"))
}

#[cfg_attr(not(any(windows, target_os = "macos")), allow(unused_imports))]
pub use codeterm_plugin::local_recording::{
    LocalInputSetupError, LocalInputSubscription, SubscriptionTeardown,
};

pub trait LocalInputAdapter: Send + Sync {
    fn subscribe(
        &self,
        declaration: &LocalRecordingTrigger,
        selected: &SelectedDevices,
        generation: u64,
        callback: std::sync::Arc<dyn Fn(SelectedInputEvent) + Send + Sync>,
    ) -> Result<Box<dyn LocalInputSubscription>, LocalInputSetupError>;
}

#[cfg(not(any(windows, target_os = "macos")))]
pub struct UnsupportedInput;

#[cfg(not(any(windows, target_os = "macos")))]
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

pub fn production_adapter() -> Arc<dyn LocalInputAdapter> {
    #[cfg(windows)]
    {
        Arc::new(WindowsInputAdapter)
    }
    #[cfg(target_os = "macos")]
    {
        Arc::new(MacInputAdapter)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Arc::new(UnsupportedInput)
    }
}

#[cfg(windows)]
struct WindowsInputAdapter;

#[cfg(windows)]
struct WindowsInputSubscription(Option<windows::WindowsInputSubscription>);

#[cfg(windows)]
impl LocalInputAdapter for WindowsInputAdapter {
    fn subscribe(
        &self,
        trigger: &LocalRecordingTrigger,
        selected: &SelectedDevices,
        generation: u64,
        callback: Arc<dyn Fn(SelectedInputEvent) + Send + Sync>,
    ) -> Result<Box<dyn LocalInputSubscription>, LocalInputSetupError> {
        if !windows::supports_trigger(trigger) {
            return Err(LocalInputSetupError {
                reason: "local_input_descriptor_unsupported".into(),
                teardown: None,
            });
        }
        let identity = windows::SelectedInputIdentity {
            input_id: selected.input_id.clone(),
            collection_id: selected.collection_id.clone(),
            trigger: trigger.clone(),
        };
        let native_callback = move |event| callback(map_windows_event(event));
        let pending =
            windows::WindowsInputSubscription::start_pending(identity, generation, native_callback)
                .map_err(|reason| LocalInputSetupError {
                    reason,
                    teardown: None,
                })?;
        match pending.wait_until_ready() {
            Ok(subscription) => Ok(Box::new(WindowsInputSubscription(Some(subscription)))),
            Err(failure) => Err(LocalInputSetupError {
                reason: windows_setup_reason(failure.reason).into(),
                teardown: Some(windows_teardown(failure.teardown)),
            }),
        }
    }
}

#[cfg(windows)]
impl LocalInputSubscription for WindowsInputSubscription {
    fn request_stop(&mut self) -> SubscriptionTeardown {
        self.0
            .take()
            .map(|subscription| windows_teardown(subscription.request_stop()))
            .unwrap_or_else(|| {
                SubscriptionTeardown::with_waiter(|_| {
                    codeterm_plugin::local_recording::SubscriptionTeardownWait::Failed(
                        "windows_input_teardown_handle_missing".into(),
                    )
                })
            })
    }
}

#[cfg(windows)]
fn map_windows_event(event: windows::WindowsInputEvent) -> SelectedInputEvent {
    match event {
        windows::WindowsInputEvent::Press { generation } => {
            SelectedInputEvent::Press { generation }
        }
        windows::WindowsInputEvent::Unavailable { generation, reason } => {
            SelectedInputEvent::Unavailable {
                generation,
                reason: match reason {
                    windows::UnavailableReason::Removed => InputUnavailable::Removed,
                    windows::UnavailableReason::AccessLost => InputUnavailable::AccessLost,
                    windows::UnavailableReason::Error => InputUnavailable::Error,
                },
            }
        }
    }
}

#[cfg(windows)]
fn windows_setup_reason(reason: windows::SetupFailure) -> &'static str {
    match reason {
        windows::SetupFailure::Unavailable(windows::UnavailableReason::Removed) => {
            "windows_input_removed"
        }
        windows::SetupFailure::Unavailable(windows::UnavailableReason::AccessLost) => {
            "windows_input_access_lost"
        }
        windows::SetupFailure::Unavailable(windows::UnavailableReason::Error) => {
            "windows_input_setup_error"
        }
        windows::SetupFailure::Stopped => "windows_input_setup_stopped",
        windows::SetupFailure::Timeout => "windows_input_setup_timeout",
        windows::SetupFailure::CompletionLost => "windows_input_setup_completion_lost",
    }
}

#[cfg(windows)]
fn windows_teardown(mut native: windows::WindowsInputTeardown) -> SubscriptionTeardown {
    SubscriptionTeardown::with_waiter(move |timeout| {
        if timeout.is_zero() {
            return codeterm_plugin::local_recording::SubscriptionTeardownWait::Pending;
        }
        match native.wait_blocking() {
            Ok(_) => codeterm_plugin::local_recording::SubscriptionTeardownWait::Complete,
            Err(error) => codeterm_plugin::local_recording::SubscriptionTeardownWait::Failed(
                format!("windows_input_teardown_failed:{error:?}"),
            ),
        }
    })
}

#[cfg(target_os = "macos")]
struct MacInputAdapter;

#[cfg(target_os = "macos")]
struct MacInputSubscription(Option<macos::MacInputSubscription>);

#[cfg(target_os = "macos")]
impl LocalInputAdapter for MacInputAdapter {
    fn subscribe(
        &self,
        declaration: &LocalRecordingTrigger,
        selected: &SelectedDevices,
        generation: u64,
        callback: Arc<dyn Fn(SelectedInputEvent) + Send + Sync>,
    ) -> Result<Box<dyn LocalInputSubscription>, LocalInputSetupError> {
        let selection = macos::MacSelection {
            input_id: selected.input_id.clone(),
            collection_id: selected.collection_id.clone(),
            vendor_id: declaration.vendor_id,
            product_id: declaration.product_id,
            collection_usage_page: declaration.usage_page,
            collection_usage: declaration.usage,
            approved_element_usage: None,
        };
        let native_callback = Arc::new(move |event| callback(map_macos_event(event)));
        macos::subscribe(selection, generation, native_callback)
            .map(|subscription| Box::new(MacInputSubscription(Some(subscription))) as _)
            .map_err(|failure| LocalInputSetupError {
                reason: failure.reason,
                teardown: failure.teardown.map(macos_teardown),
            })
    }
}

#[cfg(target_os = "macos")]
impl LocalInputSubscription for MacInputSubscription {
    fn request_stop(&mut self) -> SubscriptionTeardown {
        self.0
            .as_mut()
            .and_then(macos::MacInputSubscription::request_stop)
            .map(macos_teardown)
            .unwrap_or_else(|| {
                SubscriptionTeardown::with_waiter(|_| {
                    codeterm_plugin::local_recording::SubscriptionTeardownWait::Failed(
                        "macos_input_teardown_handle_missing".into(),
                    )
                })
            })
    }
}

#[cfg(target_os = "macos")]
fn map_macos_event(event: macos::MacInputEvent) -> SelectedInputEvent {
    match event {
        macos::MacInputEvent::Press { generation } => SelectedInputEvent::Press { generation },
        macos::MacInputEvent::Unavailable { generation, reason } => {
            SelectedInputEvent::Unavailable {
                generation,
                reason: match reason {
                    macos::MacUnavailableReason::Removed => InputUnavailable::Removed,
                    macos::MacUnavailableReason::AccessLost => InputUnavailable::AccessLost,
                    macos::MacUnavailableReason::Error => InputUnavailable::Error,
                },
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn macos_teardown(mut native: macos::MacTeardown) -> SubscriptionTeardown {
    SubscriptionTeardown::with_waiter(move |timeout| {
        if timeout.is_zero() {
            return codeterm_plugin::local_recording::SubscriptionTeardownWait::Pending;
        }
        match native.wait_blocking() {
            Ok(()) => codeterm_plugin::local_recording::SubscriptionTeardownWait::Complete,
            Err(error) => codeterm_plugin::local_recording::SubscriptionTeardownWait::Failed(error),
        }
    })
}

#[cfg(target_os = "macos")]
use codeterm_input_macos as macos;
#[cfg(windows)]
use codeterm_input_windows as windows;
