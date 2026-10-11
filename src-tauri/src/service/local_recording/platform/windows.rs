use std::collections::BTreeSet;
use std::ffi::c_void;
use std::mem::{align_of, size_of};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(test)]
use std::sync::mpsc;
use std::sync::mpsc::Sender;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

#[path = "windows/ffi.rs"]
mod ffi;
#[path = "windows/lifecycle.rs"]
mod lifecycle;
use ffi::*;
pub(super) use lifecycle::{
    receive_completion, request_stop_wake, setup_entry_result, teardown_acknowledgement,
    SetupFailure, TeardownAcknowledgement, TeardownError, ThreadControl,
    WindowsInputPendingSubscription, WindowsInputSetupFailure, WindowsInputSubscription,
    WindowsInputTeardown,
};

const SELECTED_VENDOR_PRODUCT_COLLECTION: &str = "VID_3547&PID_0008&MI_00&COL01";
const COLLECTION_ID: &str = "Col01";
const CONSUMER_CONTROL_PAGE: u16 = 0x000C;
const CONSUMER_CONTROL_USAGE: u16 = 0x0001;
const VOLUME_INCREMENT_USAGE: u16 = 0x00E9;
const BUTTON_REPORT_ID: u8 = 0x06;
const BUTTON_REPORT_BYTES: usize = 3;
const MAX_BUTTONS: usize = 8;
const MAX_INPUT_DEVICES: usize = 4096;
const MAX_DEVICE_NAME_CHARS: usize = 4096;
const MAX_PREPARSED_BYTES: usize = 65_536;
const MAX_REPORTS_PER_INPUT: usize = 8;
const MAX_REPORT_BYTES: usize = 64;
const DUPLICATE_WINDOW_MS: u64 = 40;
const HIDP_STATUS_SUCCESS: i32 = 0x0011_0000;
const RIM_TYPE_HID: u32 = 2;
const RID_INPUT: u32 = 0x1000_0003;
const RIDI_PREPARSEDDATA: u32 = 0x2000_0005;
const RIDI_DEVICENAME: u32 = 0x2000_0007;
const RIDEV_INPUTSINK: u32 = 0x0000_0100;
const RIDEV_DEVNOTIFY: u32 = 0x0000_2000;
const RIDEV_REMOVE: u32 = 0x0000_0001;
const WM_NCCREATE: u32 = 0x0081;
const WM_INPUT: u32 = 0x00FF;
const WM_INPUT_DEVICE_CHANGE: u32 = 0x00FE;
const GIDC_REMOVAL: usize = 2;
const GWLP_USERDATA: i32 = -21;
const PM_NOREMOVE: u32 = 0;
const WM_QUIT: u32 = 0x0012;
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_DEVICE_NOT_CONNECTED: u32 = 1167;

static LISTENER_CLAIMED: AtomicBool = AtomicBool::new(false);
static WINDOW_CLASS: OnceLock<Result<(), u32>> = OnceLock::new();
static WINDOW_CLASS_NAME: OnceLock<Vec<u16>> = OnceLock::new();
static CLOCK_ORIGIN: OnceLock<Instant> = OnceLock::new();

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SelectedInputIdentity {
    pub input_id: String,
    pub collection_id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum UnavailableReason {
    Removed,
    AccessLost,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum WindowsInputEvent {
    Press {
        generation: u64,
    },
    Unavailable {
        generation: u64,
        reason: UnavailableReason,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct ButtonCapability {
    usage_page: u16,
    report_id: u8,
    usage: u16,
    data_index: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct HidDescriptor {
    usage_page: u16,
    usage: u16,
    input_report_bytes: usize,
    input_button_caps: u16,
    input_value_caps: u16,
    input_data_indices: u16,
    link_collections: u16,
    button_capabilities: Vec<ButtonCapability>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DecodeResult {
    Ignored,
    Malformed,
    Press,
}

#[derive(Clone, Debug)]
struct SelectedReportDecoder {
    identity: SelectedInputIdentity,
    descriptor: HidDescriptor,
    generation: u64,
    armed: bool,
    pressed: bool,
    last_press_ms: Option<u64>,
}

impl SelectedInputIdentity {
    fn is_supported_collection(&self) -> bool {
        let path = self.input_id.to_ascii_uppercase();
        self.collection_id.eq_ignore_ascii_case(COLLECTION_ID)
            && path.starts_with(r"\\?\HID#")
            && path.contains(SELECTED_VENDOR_PRODUCT_COLLECTION)
    }

    fn matches(&self, actual: &Self) -> bool {
        self.input_id.eq_ignore_ascii_case(&actual.input_id)
            && self
                .collection_id
                .eq_ignore_ascii_case(&actual.collection_id)
    }
}

impl HidDescriptor {
    fn supported() -> Self {
        let usages = [
            0x00B7, 0x00B1, 0x00B6, 0x00B5, 0x00CD, 0x00E2, 0x00EA, 0x00E9,
        ];
        Self {
            usage_page: CONSUMER_CONTROL_PAGE,
            usage: CONSUMER_CONTROL_USAGE,
            input_report_bytes: BUTTON_REPORT_BYTES,
            input_button_caps: MAX_BUTTONS as u16,
            input_value_caps: 0,
            input_data_indices: MAX_BUTTONS as u16,
            link_collections: 1,
            button_capabilities: usages
                .into_iter()
                .enumerate()
                .map(|(index, usage)| ButtonCapability {
                    usage_page: CONSUMER_CONTROL_PAGE,
                    report_id: BUTTON_REPORT_ID,
                    usage,
                    data_index: index as u16,
                })
                .collect(),
        }
    }

    fn matches_supported_schema(&self) -> bool {
        let expected = Self::supported();
        self.usage_page == expected.usage_page
            && self.usage == expected.usage
            && self.input_report_bytes == expected.input_report_bytes
            && self.input_button_caps == expected.input_button_caps
            && self.input_value_caps == expected.input_value_caps
            && self.input_data_indices == expected.input_data_indices
            && self.link_collections == expected.link_collections
            && self
                .button_capabilities
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                == expected
                    .button_capabilities
                    .into_iter()
                    .collect::<BTreeSet<_>>()
    }
}

impl SelectedReportDecoder {
    fn new(
        identity: SelectedInputIdentity,
        descriptor: HidDescriptor,
        generation: u64,
    ) -> Option<Self> {
        (identity.is_supported_collection() && descriptor.matches_supported_schema()).then_some(
            Self {
                identity,
                descriptor,
                generation,
                armed: true,
                pressed: false,
                last_press_ms: None,
            },
        )
    }

    fn decode(
        &mut self,
        generation: u64,
        actual_identity: &SelectedInputIdentity,
        report: &[u8],
        active_usage_page: u16,
        active_usages: &[u16],
        monotonic_ms: u64,
    ) -> DecodeResult {
        if !self.armed || generation != self.generation || !self.identity.matches(actual_identity) {
            return DecodeResult::Ignored;
        }
        if !self.descriptor.matches_supported_schema()
            || report.len() != BUTTON_REPORT_BYTES
            || report.first() != Some(&BUTTON_REPORT_ID)
            || active_usage_page != CONSUMER_CONTROL_PAGE
            || active_usages.len() > MAX_BUTTONS
            || active_usages.iter().any(|usage| {
                !self
                    .descriptor
                    .button_capabilities
                    .iter()
                    .any(|capability| {
                        capability.usage_page == active_usage_page && capability.usage == *usage
                    })
            })
        {
            return DecodeResult::Malformed;
        }

        let is_pressed = active_usages.contains(&VOLUME_INCREMENT_USAGE);
        if !is_pressed {
            self.pressed = false;
            return DecodeResult::Ignored;
        }
        if self.pressed {
            return DecodeResult::Ignored;
        }
        self.pressed = true;

        if let Some(last) = self.last_press_ms {
            if monotonic_ms < last || monotonic_ms - last <= DUPLICATE_WINDOW_MS {
                return DecodeResult::Ignored;
            }
        }
        self.last_press_ms = Some(monotonic_ms);
        DecodeResult::Press
    }

    fn invalidate(&mut self, reason: UnavailableReason) -> Option<WindowsInputEvent> {
        if !self.armed {
            return None;
        }
        self.armed = false;
        self.pressed = false;
        Some(WindowsInputEvent::Unavailable {
            generation: self.generation,
            reason,
        })
    }
}

fn split_report_batch<'a>(
    report_count: u32,
    report_bytes: u32,
    payload: &'a [u8],
    expected_report_bytes: usize,
) -> Option<Vec<&'a [u8]>> {
    let count = usize::try_from(report_count).ok()?;
    let bytes = usize::try_from(report_bytes).ok()?;
    if count == 0
        || count > MAX_REPORTS_PER_INPUT
        || bytes == 0
        || bytes > MAX_REPORT_BYTES
        || bytes != expected_report_bytes
        || count.checked_mul(bytes)? != payload.len()
    {
        return None;
    }
    Some(payload.chunks_exact(bytes).collect())
}

fn unavailable_reason_for_win32(error: u32) -> UnavailableReason {
    match error {
        ERROR_DEVICE_NOT_CONNECTED => UnavailableReason::Removed,
        ERROR_ACCESS_DENIED => UnavailableReason::AccessLost,
        _ => UnavailableReason::Error,
    }
}

struct NativeListener {
    window: Hwnd,
    registered: bool,
    release_attempted: bool,
    context: Option<Box<ThreadContext>>,
}

impl NativeListener {
    fn new() -> Self {
        Self {
            window: null_mut(),
            registered: false,
            release_attempted: false,
            context: None,
        }
    }

    fn start(
        &mut self,
        identity: SelectedInputIdentity,
        generation: u64,
        callback: Arc<dyn Fn(WindowsInputEvent) + Send + Sync>,
        stop_requested: &AtomicBool,
    ) -> Result<(), ListenerStartError> {
        if stop_requested.load(Ordering::Acquire) {
            return Err(ListenerStartError::Stopped);
        }
        let selected =
            find_selected_collection(&identity).map_err(ListenerStartError::Unavailable)?;
        let descriptor = descriptor_from_preparsed(&selected.preparsed)
            .filter(|descriptor| descriptor.matches_supported_schema())
            .ok_or(ListenerStartError::Unavailable(UnavailableReason::Error))?;
        let decoder = SelectedReportDecoder::new(identity.clone(), descriptor.clone(), generation)
            .ok_or(ListenerStartError::Unavailable(UnavailableReason::Error))?;
        if stop_requested.load(Ordering::Acquire) {
            return Err(ListenerStartError::Stopped);
        }
        register_window_class()
            .map_err(unavailable_reason_for_win32)
            .map_err(ListenerStartError::Unavailable)?;
        let mut context = Box::new(ThreadContext {
            device: selected.device,
            identity,
            descriptor,
            preparsed: selected.preparsed,
            decoder,
            callback,
        });
        let class_name = wide_null("LarkSelectedRawInputWindow");
        let title = wide_null("");
        let instance = unsafe { GetModuleHandleW(null()) };
        if instance.is_null() {
            return Err(ListenerStartError::Unavailable(
                unavailable_reason_for_win32(last_error_code()),
            ));
        }
        let window = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                title.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                null_mut(),
                instance,
                (&mut *context as *mut ThreadContext).cast(),
            )
        };
        if window.is_null() {
            return Err(ListenerStartError::Unavailable(
                unavailable_reason_for_win32(last_error_code()),
            ));
        }
        self.window = window;
        self.context = Some(context);
        if stop_requested.load(Ordering::Acquire) {
            return Err(ListenerStartError::Stopped);
        }
        if has_raw_input_registration(CONSUMER_CONTROL_PAGE, CONSUMER_CONTROL_USAGE)
            .map_err(ListenerStartError::Unavailable)?
        {
            return Err(ListenerStartError::Unavailable(UnavailableReason::Error));
        }
        let registration = RawInputDevice {
            usage_page: CONSUMER_CONTROL_PAGE,
            usage: CONSUMER_CONTROL_USAGE,
            flags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
            target: window,
        };
        let registration_ok = unsafe {
            RegisterRawInputDevices(&registration, 1, size_of::<RawInputDevice>() as u32)
        };
        if registration_ok == 0 {
            return Err(ListenerStartError::Unavailable(
                unavailable_reason_for_win32(last_error_code()),
            ));
        }
        self.registered = true;
        if stop_requested.load(Ordering::Acquire) {
            return Err(ListenerStartError::Stopped);
        }
        Ok(())
    }

    fn pump(&mut self, on_entry: impl FnOnce()) -> Result<(), UnavailableReason> {
        let mut message: Msg = unsafe { std::mem::zeroed() };
        unsafe {
            PeekMessageW(&mut message, null_mut(), 0, 0, PM_NOREMOVE);
        }
        on_entry();
        loop {
            let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
            if result == 0 {
                return Ok(());
            }
            if result == -1 {
                return Err(unavailable_reason_for_win32(last_error_code()));
            }
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    fn release(&mut self) -> Result<(), TeardownError> {
        self.release_attempted = true;
        let unregister_error = if self.registered {
            let registration = RawInputDevice {
                usage_page: CONSUMER_CONTROL_PAGE,
                usage: CONSUMER_CONTROL_USAGE,
                flags: RIDEV_REMOVE,
                target: null_mut(),
            };
            if unsafe {
                RegisterRawInputDevices(&registration, 1, size_of::<RawInputDevice>() as u32)
            } != 0
            {
                self.registered = false;
                None
            } else {
                Some(last_error_code())
            }
        } else {
            None
        };
        let destroy_error = if self.window.is_null() {
            None
        } else if unsafe { DestroyWindow(self.window) } != 0 {
            self.window = null_mut();
            None
        } else {
            Some(last_error_code())
        };
        if self.window.is_null() {
            self.context.take();
        }
        if let Some(error) = unregister_error {
            return Err(TeardownError::RawInputUnregister(error));
        }
        if let Some(error) = destroy_error {
            return Err(TeardownError::WindowDestroy(error));
        }
        Ok(())
    }
}

impl Drop for NativeListener {
    fn drop(&mut self) {
        if !self.release_attempted {
            let _ = self.release();
        }
        if !self.window.is_null() {
            std::mem::forget(self.context.take());
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ListenerStartError {
    Stopped,
    Unavailable(UnavailableReason),
}

struct ThreadContext {
    device: Handle,
    identity: SelectedInputIdentity,
    descriptor: HidDescriptor,
    preparsed: AlignedPreparsedData,
    decoder: SelectedReportDecoder,
    callback: Arc<dyn Fn(WindowsInputEvent) + Send + Sync>,
}

struct SelectedCollection {
    device: Handle,
    preparsed: AlignedPreparsedData,
}

struct AlignedPreparsedData {
    words: Vec<u32>,
    bytes: usize,
}

impl AlignedPreparsedData {
    fn as_ptr(&self) -> *mut c_void {
        self.words.as_ptr().cast_mut().cast()
    }
}

fn run_listener(
    control: Arc<ThreadControl>,
    identity: SelectedInputIdentity,
    generation: u64,
    callback: Arc<dyn Fn(WindowsInputEvent) + Send + Sync>,
    startup: Sender<Result<(), SetupFailure>>,
    completion: Sender<Result<(), TeardownError>>,
) {
    let error_callback = Arc::clone(&callback);
    let mut listener = NativeListener::new();
    let mut message: Msg = unsafe { std::mem::zeroed() };
    unsafe {
        PeekMessageW(&mut message, null_mut(), 0, 0, PM_NOREMOVE);
    }
    control
        .thread_id
        .store(unsafe { GetCurrentThreadId() }, Ordering::Release);
    let start = if control.stop_requested.load(Ordering::Acquire) {
        Err(ListenerStartError::Stopped)
    } else {
        listener.start(identity, generation, callback, &control.stop_requested)
    };
    match start {
        Err(ListenerStartError::Stopped) => {
            let _ = startup.send(Err(SetupFailure::Stopped));
        }
        Err(ListenerStartError::Unavailable(reason)) => {
            let _ = startup.send(Err(SetupFailure::Unavailable(reason)));
        }
        Ok(()) => {
            if let Err(reason) = listener.pump(|| {
                let _ = startup.send(setup_entry_result(
                    control.stop_requested.load(Ordering::Acquire),
                ));
            }) {
                emit(
                    &error_callback,
                    WindowsInputEvent::Unavailable { generation, reason },
                );
            }
        }
    }
    let released = listener.release();
    control.thread_id.store(0, Ordering::Release);
    if released.is_ok() {
        LISTENER_CLAIMED.store(false, Ordering::Release);
    }
    complete_teardown(|| released, completion);
}

fn complete_teardown(
    release: impl FnOnce() -> Result<(), TeardownError>,
    completion: Sender<Result<(), TeardownError>>,
) {
    let _ = completion.send(release());
}

fn emit(callback: &Arc<dyn Fn(WindowsInputEvent) + Send + Sync>, event: WindowsInputEvent) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(event)));
}

fn find_selected_collection(
    identity: &SelectedInputIdentity,
) -> Result<SelectedCollection, UnavailableReason> {
    let mut device_count = 0u32;
    let device_size = size_of::<RawInputDeviceList>() as u32;
    let count_result = unsafe { GetRawInputDeviceList(null_mut(), &mut device_count, device_size) };
    if count_result == u32::MAX {
        return Err(unavailable_reason_for_win32(last_error_code()));
    }
    if device_count == 0 {
        return Err(UnavailableReason::Removed);
    }
    if device_count as usize > MAX_INPUT_DEVICES {
        return Err(UnavailableReason::Error);
    }
    let mut devices = vec![
        RawInputDeviceList {
            device: null_mut(),
            device_type: 0,
        };
        device_count as usize
    ];
    let listed =
        unsafe { GetRawInputDeviceList(devices.as_mut_ptr(), &mut device_count, device_size) };
    if listed == u32::MAX || listed as usize > devices.len() {
        return Err(unavailable_reason_for_win32(last_error_code()));
    }

    let mut selected = None;
    for device in devices.iter().take(listed as usize) {
        if device.device_type != RIM_TYPE_HID {
            continue;
        }
        let Ok(Some(path)) = raw_device_name(device.device) else {
            continue;
        };
        if !path.eq_ignore_ascii_case(&identity.input_id) {
            continue;
        }
        if selected.is_some() {
            return Err(UnavailableReason::Error);
        }
        let preparsed = raw_device_preparsed_data(device.device)?;
        selected = Some(SelectedCollection {
            device: device.device,
            preparsed,
        });
    }
    selected.ok_or(UnavailableReason::Removed)
}

fn raw_device_name(device: Handle) -> Result<Option<String>, UnavailableReason> {
    let mut chars = 0u32;
    let size_result =
        unsafe { GetRawInputDeviceInfoW(device, RIDI_DEVICENAME, null_mut(), &mut chars) };
    if size_result == u32::MAX {
        return Err(unavailable_reason_for_win32(last_error_code()));
    }
    if chars == 0 || chars as usize > MAX_DEVICE_NAME_CHARS {
        return Ok(None);
    }
    let mut buffer = vec![0u16; chars as usize];
    let mut capacity = chars;
    let result = unsafe {
        GetRawInputDeviceInfoW(
            device,
            RIDI_DEVICENAME,
            buffer.as_mut_ptr().cast(),
            &mut capacity,
        )
    };
    if result == u32::MAX || result as usize > buffer.len() {
        return Err(unavailable_reason_for_win32(last_error_code()));
    }
    while buffer.last() == Some(&0) {
        buffer.pop();
    }
    String::from_utf16(&buffer)
        .map(Some)
        .map_err(|_| UnavailableReason::Error)
}

fn raw_device_preparsed_data(device: Handle) -> Result<AlignedPreparsedData, UnavailableReason> {
    let mut bytes = 0u32;
    let size_result =
        unsafe { GetRawInputDeviceInfoW(device, RIDI_PREPARSEDDATA, null_mut(), &mut bytes) };
    if size_result == u32::MAX || bytes == 0 || bytes as usize > MAX_PREPARSED_BYTES {
        return Err(unavailable_reason_for_win32(last_error_code()));
    }
    let words = bytes.div_ceil(size_of::<u32>() as u32) as usize;
    let mut data = AlignedPreparsedData {
        words: vec![0; words],
        bytes: bytes as usize,
    };
    let mut capacity = bytes;
    let result =
        unsafe { GetRawInputDeviceInfoW(device, RIDI_PREPARSEDDATA, data.as_ptr(), &mut capacity) };
    if result == u32::MAX || result as usize > data.bytes {
        return Err(unavailable_reason_for_win32(last_error_code()));
    }
    Ok(data)
}

fn has_raw_input_registration(usage_page: u16, usage: u16) -> Result<bool, UnavailableReason> {
    let device_size = size_of::<RawInputDevice>() as u32;
    let mut count = 0u32;
    let result = unsafe { GetRegisteredRawInputDevices(null_mut(), &mut count, device_size) };
    if result == u32::MAX {
        return Err(unavailable_reason_for_win32(last_error_code()));
    }
    if count as usize > MAX_INPUT_DEVICES {
        return Err(UnavailableReason::Error);
    }
    if count == 0 {
        return Ok(false);
    }
    let mut devices = vec![
        RawInputDevice {
            usage_page: 0,
            usage: 0,
            flags: 0,
            target: null_mut()
        };
        count as usize
    ];
    let mut capacity = count;
    let listed =
        unsafe { GetRegisteredRawInputDevices(devices.as_mut_ptr(), &mut capacity, device_size) };
    if listed == u32::MAX || listed as usize > devices.len() {
        return Err(unavailable_reason_for_win32(last_error_code()));
    }
    Ok(devices
        .iter()
        .take(listed as usize)
        .any(|device| device.usage_page == usage_page && device.usage == usage))
}

fn descriptor_from_preparsed(data: &AlignedPreparsedData) -> Option<HidDescriptor> {
    if size_of::<HidpCaps>() != 64 || size_of::<HidpButtonCaps>() != 72 {
        return None;
    }
    let mut caps = HidpCaps::default();
    if unsafe { HidP_GetCaps(data.as_ptr(), &mut caps) } != HIDP_STATUS_SUCCESS {
        return None;
    }
    if caps.input_button_caps as usize != MAX_BUTTONS {
        return None;
    }
    let mut raw_caps =
        vec![0u32; caps.input_button_caps as usize * size_of::<HidpButtonCaps>() / 4];
    let mut cap_count = caps.input_button_caps;
    let cap_status = unsafe {
        HidP_GetButtonCaps(
            HIDP_INPUT,
            raw_caps.as_mut_ptr().cast(),
            &mut cap_count,
            data.as_ptr(),
        )
    };
    if cap_status != HIDP_STATUS_SUCCESS || cap_count as usize > MAX_BUTTONS {
        return None;
    }
    let raw_button_caps = unsafe {
        std::slice::from_raw_parts(
            raw_caps.as_ptr().cast::<HidpButtonCaps>(),
            cap_count as usize,
        )
    };
    let button_capabilities = raw_button_caps
        .iter()
        .map(|cap| unsafe { button_capability_from_raw(cap) })
        .collect::<Option<Vec<_>>>()?;
    Some(HidDescriptor {
        usage_page: caps.usage_page,
        usage: caps.usage,
        input_report_bytes: caps.input_report_bytes as usize,
        input_button_caps: caps.input_button_caps,
        input_value_caps: caps.input_value_caps,
        input_data_indices: caps.input_data_indices,
        link_collections: caps.link_collections,
        button_capabilities,
    })
}

unsafe fn button_capability_from_raw(raw: &HidpButtonCaps) -> Option<ButtonCapability> {
    if raw.is_alias != 0
        || raw.bit_field != 0x0042
        || raw.link_collection != 0
        || raw.link_usage != CONSUMER_CONTROL_USAGE
        || raw.link_usage_page != CONSUMER_CONTROL_PAGE
        || raw.is_range != 0
        || raw.is_absolute == 0
        || raw.report_count != 1
    {
        return None;
    }
    let usage = unsafe { raw.data.not_range };
    Some(ButtonCapability {
        usage_page: raw.usage_page,
        report_id: raw.report_id,
        usage: usage.usage,
        data_index: usage.data_index,
    })
}

unsafe fn parse_active_usages(preparsed: *mut c_void, report: &[u8]) -> Option<Vec<u16>> {
    if report.len() != BUTTON_REPORT_BYTES {
        return None;
    }
    let mut usages = [0u16; MAX_BUTTONS];
    let mut usage_count = usages.len() as u32;
    let status = HidP_GetUsages(
        HIDP_INPUT,
        CONSUMER_CONTROL_PAGE,
        0,
        usages.as_mut_ptr(),
        &mut usage_count,
        preparsed,
        report.as_ptr().cast::<i8>().cast_mut(),
        report.len() as u32,
    );
    (status == HIDP_STATUS_SUCCESS && usage_count as usize <= usages.len())
        .then(|| usages[..usage_count as usize].to_vec())
}

fn process_raw_input(raw_input: Handle, context: &mut ThreadContext) {
    let Some(packet) = read_raw_input(raw_input) else {
        return;
    };
    if packet.header.device != context.device || packet.header.device.is_null() {
        return;
    }
    if packet.header.device_type != RIM_TYPE_HID {
        return;
    }
    let Some((report_count, report_bytes, payload)) = packet.selected_hid_payload() else {
        return;
    };
    let Some(reports) = split_report_batch(
        report_count,
        report_bytes,
        &payload,
        context.descriptor.input_report_bytes,
    ) else {
        return;
    };
    for report in reports {
        let Some(usages) = (unsafe { parse_active_usages(context.preparsed.as_ptr(), report) })
        else {
            continue;
        };
        let result = context.decoder.decode(
            context.decoder.generation,
            &context.identity,
            report,
            CONSUMER_CONTROL_PAGE,
            &usages,
            monotonic_millis(),
        );
        if result == DecodeResult::Press {
            emit(
                &context.callback,
                WindowsInputEvent::Press {
                    generation: context.decoder.generation,
                },
            );
        }
    }
}

struct RawInputPacket {
    header: RawInputHeader,
    storage: Vec<u64>,
    size: usize,
}

impl RawInputPacket {
    fn selected_hid_payload(&self) -> Option<(u32, u32, Vec<u8>)> {
        let buffer = self.storage.as_ptr().cast::<u8>();
        let hid = unsafe {
            std::ptr::read_unaligned(
                buffer
                    .add(size_of::<RawInputHeader>())
                    .cast::<RawHidHeader>(),
            )
        };
        let payload_len = (hid.report_bytes as usize).checked_mul(hid.report_count as usize)?;
        let payload_start = size_of::<RawInputHeader>() + size_of::<RawHidHeader>();
        let payload_end = payload_start.checked_add(payload_len)?;
        if payload_end > self.size || self.size - payload_end >= align_of::<usize>() {
            return None;
        }
        let payload = unsafe { std::slice::from_raw_parts(buffer.add(payload_start), payload_len) };
        Some((hid.report_count, hid.report_bytes, payload.to_vec()))
    }
}

fn read_raw_input(raw_input: Handle) -> Option<RawInputPacket> {
    let mut required = 0u32;
    let header_bytes = size_of::<RawInputHeader>() as u32;
    let result = unsafe {
        GetRawInputData(
            raw_input,
            RID_INPUT,
            null_mut(),
            &mut required,
            header_bytes,
        )
    };
    if result == u32::MAX
        || (required as usize) < size_of::<RawInputHeader>() + size_of::<RawHidHeader>()
        || required as usize
            > size_of::<RawInputHeader>()
                + size_of::<RawHidHeader>()
                + MAX_REPORTS_PER_INPUT * MAX_REPORT_BYTES
                + align_of::<usize>()
    {
        return None;
    }
    let words = required.div_ceil(size_of::<u64>() as u32) as usize;
    let mut storage = vec![0u64; words];
    let mut capacity = required;
    let result = unsafe {
        GetRawInputData(
            raw_input,
            RID_INPUT,
            storage.as_mut_ptr().cast(),
            &mut capacity,
            header_bytes,
        )
    };
    if result == u32::MAX || result != required || capacity != required {
        return None;
    }
    let buffer = storage.as_ptr().cast::<u8>();
    let header = unsafe { std::ptr::read_unaligned(buffer.cast::<RawInputHeader>()) };
    if header.size as usize != required as usize {
        return None;
    }
    Some(RawInputPacket {
        header,
        storage,
        size: required as usize,
    })
}

fn monotonic_millis() -> u64 {
    CLOCK_ORIGIN
        .get_or_init(Instant::now)
        .elapsed()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn register_window_class() -> Result<(), u32> {
    *WINDOW_CLASS.get_or_init(|| unsafe {
        let class_name = WINDOW_CLASS_NAME.get_or_init(|| wide_null("LarkSelectedRawInputWindow"));
        let instance = GetModuleHandleW(null());
        if instance.is_null() {
            return Err(last_error_code());
        }
        let window_class = WndClassW {
            style: 0,
            procedure: Some(window_proc),
            class_extra: 0,
            window_extra: 0,
            instance,
            icon: null_mut(),
            cursor: null_mut(),
            background: null_mut(),
            menu_name: null(),
            class_name: class_name.as_ptr(),
        };
        if RegisterClassW(&window_class) == 0 {
            let error = last_error_code();
            if error != 1410 {
                return Err(error);
            }
        }
        Ok(())
    })
}

fn emit_unavailable(context: &mut ThreadContext, reason: UnavailableReason) {
    if let Some(event) = context.decoder.invalidate(reason) {
        emit(&context.callback, event);
    }
}

unsafe extern "system" fn window_proc(
    window: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
) -> Lresult {
    if message == WM_NCCREATE {
        let create = &*(lparam as *const CreateStructW);
        SetWindowLongPtrW(window, GWLP_USERDATA, create.parameter as isize);
        return 1;
    }
    let context = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut ThreadContext;
    if context.is_null() {
        return DefWindowProcW(window, message, wparam, lparam);
    }
    match message {
        WM_INPUT => process_raw_input(lparam as Handle, &mut *context),
        WM_INPUT_DEVICE_CHANGE if wparam == GIDC_REMOVAL => {
            let removed = lparam as Handle;
            if removed == (*context).device {
                emit_unavailable(&mut *context, UnavailableReason::Removed);
                PostQuitMessage(0);
            }
        }
        _ => {}
    }
    DefWindowProcW(window, message, wparam, lparam)
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_error_code() -> u32 {
    std::io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or_default() as u32
}

#[cfg(test)]
#[path = "windows/tests.rs"]
mod tests;
