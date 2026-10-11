#[cfg(target_os = "macos")]
use std::sync::Arc;

const MAX_DESCRIPTOR_ELEMENTS: usize = 512;
const MAX_COLLECTION_DEPTH: usize = 32;
const DEVICE_ID_PREFIX: &str = "macos-hid-v1:";
const COLLECTION_ID_PREFIX: &str = "macos-hid-collection-v1:";

pub(crate) trait RunLoopResourceOps: Send {
    fn stop_and_wake(&mut self);
    fn release(&mut self);
}

pub(crate) struct RunLoopHandle {
    operations: Option<Box<dyn RunLoopResourceOps>>,
    stop_requested: bool,
}

impl RunLoopHandle {
    pub(crate) fn new(operations: impl RunLoopResourceOps + 'static) -> Self {
        Self {
            operations: Some(Box::new(operations)),
            stop_requested: false,
        }
    }

    pub(crate) fn stop(&mut self) {
        if self.stop_requested {
            return;
        }
        self.stop_requested = true;
        if let Some(operations) = self.operations.as_mut() {
            operations.stop_and_wake();
        }
    }
}

pub(crate) fn send_ready_run_loop(
    sender: std::sync::mpsc::SyncSender<Result<RunLoopHandle, String>>,
    run_loop: RunLoopHandle,
) {
    if let Err(error) = sender.send(Ok(run_loop)) {
        drop(error.0);
    }
}

pub(crate) fn abandon_ready_run_loop(
    sender: std::sync::mpsc::SyncSender<Result<RunLoopHandle, String>>,
    reason: &'static str,
    run_loop: Option<RunLoopHandle>,
) {
    let _ = sender.send(Err(reason.into()));
    drop(run_loop);
}

impl Drop for RunLoopHandle {
    fn drop(&mut self) {
        self.stop();
        if let Some(mut operations) = self.operations.take() {
            operations.release();
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MacInputEvent {
    Press {
        generation: u64,
    },
    Unavailable {
        generation: u64,
        reason: MacUnavailableReason,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MacUnavailableReason {
    Removed,
    AccessLost,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MacSelection {
    pub input_id: String,
    pub collection_id: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub collection_usage_page: u16,
    pub collection_usage: u16,
    pub approved_element_usage: Option<MacElementUsage>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MacElementUsage {
    pub usage_page: u16,
    pub usage: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DeviceIdentity {
    location_id: u32,
    registry_entry_id: u64,
}
impl DeviceIdentity {
    fn encode(self) -> String {
        format!(
            "{DEVICE_ID_PREFIX}{:08x}:{:016x}",
            self.location_id, self.registry_entry_id
        )
    }
    fn decode(value: &str) -> Option<Self> {
        let value = value.strip_prefix(DEVICE_ID_PREFIX)?;
        let (location_id, registry_entry_id) = value.split_once(':')?;
        Some(Self {
            location_id: u32::from_str_radix(location_id, 16).ok()?,
            registry_entry_id: u64::from_str_radix(registry_entry_id, 16).ok()?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CollectionIdentity {
    cookie: u32,
    usage_page: u16,
    usage: u16,
}

impl CollectionIdentity {
    fn decode(value: &str) -> Option<Self> {
        let value = value.strip_prefix(COLLECTION_ID_PREFIX)?;
        let (cookie, remainder) = value.split_once(':')?;
        let (usage_page, usage) = remainder.split_once(':')?;
        Some(Self {
            cookie: u32::from_str_radix(cookie, 16).ok()?,
            usage_page: u16::from_str_radix(usage_page, 16).ok()?,
            usage: u16::from_str_radix(usage, 16).ok()?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ElementKind {
    Button,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ElementDescriptor {
    cookie: u32,
    usage_page: u16,
    usage: u16,
    kind: ElementKind,
    logical_min: i64,
    logical_max: i64,
    report_size: u32,
    report_count: u32,
    is_array: bool,
    is_relative: bool,
    application_collections: Vec<CollectionIdentity>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ApprovedElement {
    cookie: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DescriptorError {
    WrongDevice,
    WrongCollection,
    MissingApprovedMapping,
    MissingElement,
    AmbiguousElement,
    InvalidElement,
}

fn selected_collection(selection: &MacSelection) -> Result<CollectionIdentity, DescriptorError> {
    let collection = CollectionIdentity::decode(&selection.collection_id)
        .ok_or(DescriptorError::WrongCollection)?;
    if collection.cookie == 0
        || collection.usage_page != selection.collection_usage_page
        || collection.usage != selection.collection_usage
    {
        return Err(DescriptorError::WrongCollection);
    }
    Ok(collection)
}

fn setup_validation_error(selection: &MacSelection) -> Option<&'static str> {
    if DeviceIdentity::decode(&selection.input_id).is_none()
        || selected_collection(selection).is_err()
    {
        Some("macos_input_selection_invalid")
    } else if selection.approved_element_usage.is_none() {
        Some("macos_hid_element_mapping_unavailable")
    } else {
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TeardownWaitOutcome {
    Released,
    Pending,
    Failed(String),
}

fn classify_teardown_wait(
    result: Result<Result<(), String>, std::sync::mpsc::RecvTimeoutError>,
) -> TeardownWaitOutcome {
    match result {
        Ok(Ok(())) => TeardownWaitOutcome::Released,
        Ok(Err(error)) => TeardownWaitOutcome::Failed(error),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => TeardownWaitOutcome::Pending,
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            TeardownWaitOutcome::Failed("macos_input_teardown_unacknowledged".into())
        }
    }
}

fn teardown_reservation_required(outcome: &TeardownWaitOutcome) -> bool {
    !matches!(outcome, TeardownWaitOutcome::Released)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AccessCheck {
    Granted,
    Denied,
    Unknown,
}
fn access_failure_reason(access: AccessCheck) -> MacUnavailableReason {
    match access {
        AccessCheck::Denied | AccessCheck::Unknown => MacUnavailableReason::AccessLost,
        AccessCheck::Granted => MacUnavailableReason::Error,
    }
}
fn native_error(message: &str, reason: MacUnavailableReason) -> (String, MacUnavailableReason) {
    (message.into(), reason)
}

fn approve_descriptor(
    selection: &MacSelection,
    observed_device_id: &str,
    elements: &[ElementDescriptor],
) -> Result<ApprovedElement, DescriptorError> {
    let identity =
        DeviceIdentity::decode(&selection.input_id).ok_or(DescriptorError::WrongDevice)?;
    if identity.encode() != observed_device_id {
        return Err(DescriptorError::WrongDevice);
    }
    let selected_collection = selected_collection(selection)?;
    let approved_usage = selection
        .approved_element_usage
        .ok_or(DescriptorError::MissingApprovedMapping)?;
    let matching: Vec<_> = elements
        .iter()
        .filter(|element| {
            element.kind == ElementKind::Button
                && element.usage_page == approved_usage.usage_page
                && element.usage == approved_usage.usage
                && element
                    .application_collections
                    .contains(&selected_collection)
        })
        .collect();
    let [element] = matching.as_slice() else {
        return Err(if matching.is_empty() {
            DescriptorError::MissingElement
        } else {
            DescriptorError::AmbiguousElement
        });
    };
    if element.cookie == 0
        || element.logical_min != 0
        || element.logical_max != 1
        || element.report_size != 1
        || element.report_count != 1
        || element.is_array
        || element.is_relative
    {
        return Err(DescriptorError::InvalidElement);
    }
    Ok(ApprovedElement {
        cookie: element.cookie,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InputDecoder {
    generation: u64,
    input_id: String,
    collection_id: String,
    element: ApprovedElement,
    pressed: bool,
    active: bool,
}

impl InputDecoder {
    fn new(selection: &MacSelection, element: ApprovedElement, generation: u64) -> Self {
        Self {
            generation,
            input_id: selection.input_id.clone(),
            collection_id: selection.collection_id.clone(),
            element,
            pressed: false,
            active: true,
        }
    }
    fn observe(
        &mut self,
        input_id: &str,
        collection_id: &str,
        element_cookie: u32,
        value: i64,
    ) -> Option<MacInputEvent> {
        if !self.active
            || input_id != self.input_id
            || collection_id != self.collection_id
            || element_cookie != self.element.cookie
        {
            return None;
        }
        match value {
            0 => {
                self.pressed = false;
                None
            }
            1 if !self.pressed => {
                self.pressed = true;
                Some(MacInputEvent::Press {
                    generation: self.generation,
                })
            }
            1 => None,
            _ => None,
        }
    }

    fn unavailable(&mut self, reason: MacUnavailableReason) -> Option<MacInputEvent> {
        if !self.active {
            return None;
        }
        self.active = false;
        Some(MacInputEvent::Unavailable {
            generation: self.generation,
            reason,
        })
    }
}
#[cfg(target_os = "macos")]
pub(crate) use native::{MacInputSubscription, MacSetupFailure, MacTeardown};
#[cfg(target_os = "macos")]
pub(crate) fn subscribe(
    selection: MacSelection,
    generation: u64,
    callback: Arc<dyn Fn(MacInputEvent) + Send + Sync>,
) -> Result<MacInputSubscription, MacSetupFailure> {
    native::subscribe(selection, generation, callback)
}

#[cfg(target_os = "macos")]
mod native;
#[cfg(test)]
#[path = "macos/tests.rs"]
mod tests;
