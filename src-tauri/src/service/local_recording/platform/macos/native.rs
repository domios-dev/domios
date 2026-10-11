use super::{
    access_failure_reason, approve_descriptor, native_error, setup_validation_error, AccessCheck,
    DeviceIdentity, ElementDescriptor, ElementKind, InputDecoder, MacInputEvent, MacSelection,
    MacUnavailableReason, MAX_COLLECTION_DEPTH, MAX_DESCRIPTOR_ELEMENTS,
};
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

type CFIndex = isize;
type CFTypeRef = *const c_void;
type CFAllocatorRef = CFTypeRef;
type CFStringRef = CFTypeRef;
type CFDictionaryRef = CFTypeRef;
type CFMutableDictionaryRef = *mut c_void;
type CFArrayRef = CFTypeRef;
type CFSetRef = CFTypeRef;
type CFNumberRef = CFTypeRef;
type CFStringEncoding = u32;
type CFRunLoopRef = *mut c_void;
type CFRunLoopObserverRef = *mut c_void;
type IOHIDManagerRef = *mut c_void;
type IOHIDDeviceRef = *mut c_void;
type IOHIDElementRef = *mut c_void;
type IOHIDValueRef = *mut c_void;
type IOReturn = i32;
const CF_NUMBER_SINT64_TYPE: i32 = 4;
const CF_STRING_ENCODING_UTF8: CFStringEncoding = 0x0800_0100;
const CF_RUN_LOOP_ENTRY: usize = 1;
const SETUP_ACK_TIMEOUT: Duration = Duration::from_secs(5);
const TEARDOWN_ACK_TIMEOUT: Duration = Duration::from_secs(5);
const IOHID_REQUEST_LISTEN_EVENT: u32 = 1;
const IOHID_ACCESS_GRANTED: u32 = 0;
const IOHID_ACCESS_DENIED: u32 = 1;
const IOHID_ACCESS_UNKNOWN: u32 = 2;
const IOHID_ELEMENT_TYPE_INPUT_BUTTON: u32 = 2;
const IOHID_ELEMENT_TYPE_COLLECTION: u32 = 513;
const IOHID_COLLECTION_TYPE_APPLICATION: u32 = 1;
const MAX_MATCHING_DEVICES: usize = 64;
type ValueCallback = unsafe extern "C" fn(*mut c_void, IOReturn, *mut c_void, IOHIDValueRef);
type DeviceCallback = unsafe extern "C" fn(*mut c_void, IOReturn, *mut c_void, IOHIDDeviceRef);
type RunLoopObserverCallback = unsafe extern "C" fn(CFRunLoopObserverRef, usize, *mut c_void);
#[repr(C)]
struct CFDictionaryKeyCallBacks([usize; 6]);
#[repr(C)]
struct CFDictionaryValueCallBacks([usize; 5]);
#[repr(C)]
struct CFRunLoopObserverContext([usize; 5]);

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFAllocatorDefault: CFAllocatorRef;
    static kCFRunLoopDefaultMode: CFStringRef;
    static kCFTypeDictionaryKeyCallBacks: CFDictionaryKeyCallBacks;
    static kCFTypeDictionaryValueCallBacks: CFDictionaryValueCallBacks;
    fn CFRelease(value: CFTypeRef);
    fn CFRetain(value: CFTypeRef) -> CFTypeRef;
    fn CFStringCreateWithCString(
        allocator: CFAllocatorRef,
        value: *const c_char,
        encoding: CFStringEncoding,
    ) -> CFStringRef;
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopRun();
    fn CFRunLoopObserverCreate(
        allocator: CFAllocatorRef,
        activities: usize,
        repeats: u8,
        order: CFIndex,
        callout: Option<RunLoopObserverCallback>,
        context: *const CFRunLoopObserverContext,
    ) -> CFRunLoopObserverRef;
    fn CFRunLoopAddObserver(
        run_loop: CFRunLoopRef,
        observer: CFRunLoopObserverRef,
        mode: CFStringRef,
    );
    fn CFRunLoopObserverInvalidate(observer: CFRunLoopObserverRef);
    fn CFRunLoopStop(run_loop: CFRunLoopRef);
    fn CFRunLoopWakeUp(run_loop: CFRunLoopRef);
    fn CFNumberCreate(
        allocator: CFAllocatorRef,
        number_type: i32,
        value: *const c_void,
    ) -> CFNumberRef;
    fn CFNumberGetValue(number: CFNumberRef, number_type: i32, value: *mut c_void) -> u8;
    fn CFNumberGetTypeID() -> usize;
    fn CFGetTypeID(value: CFTypeRef) -> usize;
    fn CFDictionaryCreateMutable(
        allocator: CFAllocatorRef,
        capacity: CFIndex,
        key_callbacks: *const CFDictionaryKeyCallBacks,
        value_callbacks: *const CFDictionaryValueCallBacks,
    ) -> CFMutableDictionaryRef;
    fn CFDictionarySetValue(dictionary: CFMutableDictionaryRef, key: CFTypeRef, value: CFTypeRef);
    fn CFArrayGetCount(array: CFArrayRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(array: CFArrayRef, index: CFIndex) -> CFTypeRef;
    fn CFSetGetCount(set: CFSetRef) -> CFIndex;
    fn CFSetGetValues(set: CFSetRef, values: *mut CFTypeRef);
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDCheckAccess(request_type: u32) -> u32;
    fn IOHIDRequestAccess(request_type: u32) -> u8;
    fn IOHIDManagerCreate(allocator: CFAllocatorRef, options: u32) -> IOHIDManagerRef;
    fn IOHIDManagerSetDeviceMatching(manager: IOHIDManagerRef, matching: CFDictionaryRef);
    fn IOHIDManagerRegisterDeviceRemovalCallback(
        manager: IOHIDManagerRef,
        callback: Option<DeviceCallback>,
        context: *mut c_void,
    );
    fn IOHIDManagerRegisterInputValueCallback(
        manager: IOHIDManagerRef,
        callback: Option<ValueCallback>,
        context: *mut c_void,
    );
    fn IOHIDManagerSetInputValueMatching(manager: IOHIDManagerRef, matching: CFDictionaryRef);
    fn IOHIDManagerScheduleWithRunLoop(
        manager: IOHIDManagerRef,
        run_loop: CFRunLoopRef,
        run_loop_mode: CFStringRef,
    );
    fn IOHIDManagerUnscheduleFromRunLoop(
        manager: IOHIDManagerRef,
        run_loop: CFRunLoopRef,
        run_loop_mode: CFStringRef,
    );
    fn IOHIDManagerOpen(manager: IOHIDManagerRef, options: u32) -> IOReturn;
    fn IOHIDManagerClose(manager: IOHIDManagerRef, options: u32) -> IOReturn;
    fn IOHIDManagerCopyDevices(manager: IOHIDManagerRef) -> CFSetRef;
    fn IOHIDDeviceGetProperty(device: IOHIDDeviceRef, key: CFStringRef) -> CFTypeRef;
    fn IOHIDDeviceGetService(device: IOHIDDeviceRef) -> u32;
    fn IOHIDDeviceCopyMatchingElements(
        device: IOHIDDeviceRef,
        matching: CFDictionaryRef,
        options: u32,
    ) -> CFArrayRef;
    fn IOHIDValueGetElement(value: IOHIDValueRef) -> IOHIDElementRef;
    fn IOHIDValueGetIntegerValue(value: IOHIDValueRef) -> CFIndex;
    fn IOHIDElementGetDevice(element: IOHIDElementRef) -> IOHIDDeviceRef;
    fn IOHIDElementGetParent(element: IOHIDElementRef) -> IOHIDElementRef;
    fn IOHIDElementGetType(element: IOHIDElementRef) -> u32;
    fn IOHIDElementGetCollectionType(element: IOHIDElementRef) -> u32;
    fn IOHIDElementGetCookie(element: IOHIDElementRef) -> u32;
    fn IOHIDElementGetReportSize(element: IOHIDElementRef) -> u32;
    fn IOHIDElementGetReportCount(element: IOHIDElementRef) -> u32;
    fn IOHIDElementGetUsagePage(element: IOHIDElementRef) -> u32;
    fn IOHIDElementGetUsage(element: IOHIDElementRef) -> u32;
    fn IOHIDElementGetLogicalMin(element: IOHIDElementRef) -> CFIndex;
    fn IOHIDElementGetLogicalMax(element: IOHIDElementRef) -> CFIndex;
    fn IOHIDElementIsArray(element: IOHIDElementRef) -> u8;
    fn IOHIDElementIsRelative(element: IOHIDElementRef) -> u8;
    fn IORegistryEntryGetRegistryEntryID(entry: u32, entry_id: *mut u64) -> IOReturn;
}

pub(crate) struct MacInputSubscription {
    active: Arc<AtomicBool>,
    run_loop: Option<RunLoopHandle>,
    completion: Option<Receiver<Result<(), String>>>,
}

pub(crate) struct MacSetupFailure {
    pub(crate) reason: String,
    pub(crate) teardown: Option<MacTeardown>,
}
pub(crate) struct MacTeardown {
    completion: Receiver<Result<(), String>>,
    run_loop: Option<RunLoopHandle>,
    state: TeardownState,
}
enum TeardownState {
    Pending,
    Complete,
    Failed(String),
}

impl MacTeardown {
    pub(crate) fn wait_blocking(&mut self) -> Result<(), String> {
        match &self.state {
            TeardownState::Complete => return Ok(()),
            TeardownState::Failed(error) => return Err(error.clone()),
            TeardownState::Pending => {}
        }
        let outcome =
            super::classify_teardown_wait(self.completion.recv_timeout(TEARDOWN_ACK_TIMEOUT));
        if super::teardown_reservation_required(&outcome) {
            return match outcome {
                super::TeardownWaitOutcome::Pending => Err("macos_input_teardown_timeout".into()),
                super::TeardownWaitOutcome::Failed(error) => {
                    self.state = TeardownState::Failed(error.clone());
                    Err(error)
                }
                super::TeardownWaitOutcome::Released => unreachable!(),
            };
        }
        self.run_loop.take();
        self.state = TeardownState::Complete;
        Ok(())
    }
}

struct RunLoopHandle(CFRunLoopRef);
unsafe impl Send for RunLoopHandle {}
impl RunLoopHandle {
    fn stop(&self) {
        unsafe {
            CFRunLoopStop(self.0);
            CFRunLoopWakeUp(self.0);
        }
    }
}

impl Drop for RunLoopHandle {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) };
    }
}
impl MacInputSubscription {
    pub(crate) fn request_stop(&mut self) -> Option<MacTeardown> {
        self.active.store(false, Ordering::Release);
        if let Some(run_loop) = &self.run_loop {
            run_loop.stop();
        }
        self.completion.take().map(|completion| MacTeardown {
            completion,
            run_loop: self.run_loop.take(),
            state: TeardownState::Pending,
        })
    }
}
impl Drop for MacInputSubscription {
    fn drop(&mut self) {
        if self.completion.is_some() {
            self.active.store(false, Ordering::Release);
            if let Some(run_loop) = &self.run_loop {
                run_loop.stop();
            }
            self.completion.take();
        }
    }
}

struct NativeState {
    selection: MacSelection,
    generation: u64,
    callback: Arc<dyn Fn(MacInputEvent) + Send + Sync>,
    active: Arc<AtomicBool>,
    decoder: Option<InputDecoder>,
    run_loop: CFRunLoopRef,
    selected_device: IOHIDDeviceRef,
    requires_input_monitoring: bool,
}

struct RunLoopStart {
    ready: Option<SyncSender<Result<usize, String>>>,
    run_loop: CFRunLoopRef,
    active: Arc<AtomicBool>,
}
unsafe extern "C" fn run_loop_started(
    _observer: CFRunLoopObserverRef,
    _activity: usize,
    info: *mut c_void,
) {
    let start = &mut *info.cast::<RunLoopStart>();
    let Some(ready) = start.ready.take() else {
        return;
    };
    if !start.active.load(Ordering::Acquire) {
        let _ = ready.send(Err("macos_input_setup_cancelled".into()));
        CFRunLoopStop(start.run_loop);
        CFRunLoopWakeUp(start.run_loop);
        CFRelease(start.run_loop);
        return;
    }
    if ready.send(Ok(start.run_loop as usize)).is_err() {
        CFRunLoopStop(start.run_loop);
        CFRunLoopWakeUp(start.run_loop);
        CFRelease(start.run_loop);
    }
}

impl NativeState {
    fn emit_unavailable(&mut self, reason: MacUnavailableReason) {
        if self.active.swap(false, Ordering::AcqRel) {
            let event = self
                .decoder
                .as_mut()
                .and_then(|decoder| decoder.unavailable(reason.clone()))
                .unwrap_or(MacInputEvent::Unavailable {
                    generation: self.generation,
                    reason,
                });
            (self.callback)(event);
        }
        unsafe {
            CFRunLoopStop(self.run_loop);
            CFRunLoopWakeUp(self.run_loop);
        }
    }
    fn input_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
}

impl Drop for NativeState {
    fn drop(&mut self) {
        if !self.selected_device.is_null() {
            unsafe { CFRelease(self.selected_device) };
        }
    }
}

struct ManagerResources {
    manager: IOHIDManagerRef,
    run_loop: CFRunLoopRef,
    scheduled: bool,
    opened: bool,
}
impl ManagerResources {
    unsafe fn release(&mut self, context: *mut c_void) -> Result<(), String> {
        if self.manager.is_null() {
            return Ok(());
        }
        IOHIDManagerRegisterDeviceRemovalCallback(self.manager, None, context);
        IOHIDManagerRegisterInputValueCallback(self.manager, None, context);
        let close_failed = self.opened && IOHIDManagerClose(self.manager, 0) != 0;
        if self.scheduled {
            IOHIDManagerUnscheduleFromRunLoop(self.manager, self.run_loop, kCFRunLoopDefaultMode);
        }
        CFRelease(self.manager);
        self.manager = std::ptr::null_mut();
        if close_failed {
            Err("macos_hid_manager_close_failed".into())
        } else {
            Ok(())
        }
    }
}

fn fail_setup(
    mut state: Box<NativeState>,
    mut resources: ManagerResources,
    context: *mut c_void,
    error: String,
    reason: MacUnavailableReason,
    ready: SyncSender<Result<usize, String>>,
    done: SyncSender<Result<(), String>>,
) {
    state.emit_unavailable(reason);
    let _ = ready.send(Err(error));
    let cleanup = unsafe { resources.release(context) };
    drop(state);
    let _ = done.send(cleanup);
}

pub(crate) fn subscribe(
    selection: MacSelection,
    generation: u64,
    callback: Arc<dyn Fn(MacInputEvent) + Send + Sync>,
) -> Result<MacInputSubscription, MacSetupFailure> {
    if let Some(reason) = setup_validation_error(&selection) {
        return Err(MacSetupFailure {
            reason: reason.into(),
            teardown: None,
        });
    }
    let active = Arc::new(AtomicBool::new(true));
    let worker_active = active.clone();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("lark-macos-hid".into())
        .spawn(move || {
            run_session(
                selection,
                generation,
                callback,
                worker_active,
                ready_tx,
                done_tx,
            );
        })
        .map_err(|_| MacSetupFailure {
            reason: "macos_input_thread_unavailable".into(),
            teardown: None,
        })?;

    let run_loop = match ready_rx.recv_timeout(SETUP_ACK_TIMEOUT) {
        Ok(Ok(run_loop)) => run_loop,
        Ok(Err(reason)) => {
            return Err(MacSetupFailure {
                reason,
                teardown: Some(MacTeardown {
                    completion: done_rx,
                    run_loop: None,
                    state: TeardownState::Pending,
                }),
            });
        }
        Err(RecvTimeoutError::Timeout) => {
            active.store(false, Ordering::Release);
            return Err(MacSetupFailure {
                reason: "macos_input_setup_timeout".into(),
                teardown: Some(MacTeardown {
                    completion: done_rx,
                    run_loop: None,
                    state: TeardownState::Pending,
                }),
            });
        }
        Err(RecvTimeoutError::Disconnected) => {
            active.store(false, Ordering::Release);
            return Err(MacSetupFailure {
                reason: "macos_input_start_unacknowledged".into(),
                teardown: Some(MacTeardown {
                    completion: done_rx,
                    run_loop: None,
                    state: TeardownState::Pending,
                }),
            });
        }
    };
    Ok(MacInputSubscription {
        active,
        run_loop: Some(RunLoopHandle(run_loop as CFRunLoopRef)),
        completion: Some(done_rx),
    })
}

fn run_session(
    selection: MacSelection,
    generation: u64,
    callback: Arc<dyn Fn(MacInputEvent) + Send + Sync>,
    active: Arc<AtomicBool>,
    ready: SyncSender<Result<usize, String>>,
    done: SyncSender<Result<(), String>>,
) {
    let run_loop = unsafe { CFRunLoopGetCurrent() };
    if run_loop.is_null() {
        callback(MacInputEvent::Unavailable {
            generation,
            reason: MacUnavailableReason::Error,
        });
        let _ = ready.send(Err("macos_run_loop_unavailable".into()));
        let _ = done.send(Ok(()));
        return;
    }

    let manager = unsafe { IOHIDManagerCreate(kCFAllocatorDefault, 0) };
    if manager.is_null() {
        callback(MacInputEvent::Unavailable {
            generation,
            reason: MacUnavailableReason::Error,
        });
        let _ = ready.send(Err("macos_hid_manager_unavailable".into()));
        let _ = done.send(Ok(()));
        return;
    }

    let mut resources = ManagerResources {
        manager,
        run_loop,
        scheduled: false,
        opened: false,
    };
    let mut state = Box::new(NativeState {
        selection,
        generation,
        callback,
        active,
        decoder: None,
        run_loop,
        selected_device: std::ptr::null_mut(),
        requires_input_monitoring: false,
    });
    let context = (&mut *state as *mut NativeState).cast::<c_void>();
    let setup = unsafe { prepare_manager(&mut resources, context, &mut state) };
    if let Err((error, reason)) = setup {
        return fail_setup(state, resources, context, error, reason, ready, done);
    }
    if !state.input_active() {
        return fail_setup(
            state,
            resources,
            context,
            "macos_input_setup_cancelled".into(),
            MacUnavailableReason::Error,
            ready,
            done,
        );
    }

    let retained_run_loop = unsafe { CFRetain(run_loop as CFTypeRef) as CFRunLoopRef };
    if retained_run_loop.is_null() {
        return fail_setup(
            state,
            resources,
            context,
            "macos_run_loop_retain_failed".into(),
            MacUnavailableReason::Error,
            ready,
            done,
        );
    }
    let mut start = RunLoopStart {
        ready: Some(ready),
        run_loop: retained_run_loop,
        active: state.active.clone(),
    };
    let observer_context =
        CFRunLoopObserverContext([0, (&mut start as *mut RunLoopStart) as usize, 0, 0, 0]);
    let observer = unsafe {
        CFRunLoopObserverCreate(
            kCFAllocatorDefault,
            CF_RUN_LOOP_ENTRY,
            0,
            0,
            Some(run_loop_started),
            &observer_context,
        )
    };
    if observer.is_null() {
        let ready = start.ready.take().unwrap();
        unsafe { CFRelease(retained_run_loop) };
        return fail_setup(
            state,
            resources,
            context,
            "macos_run_loop_observer_unavailable".into(),
            MacUnavailableReason::Error,
            ready,
            done,
        );
    }
    unsafe { CFRunLoopAddObserver(run_loop, observer, kCFRunLoopDefaultMode) };

    unsafe { CFRunLoopRun() };
    unsafe {
        CFRunLoopObserverInvalidate(observer);
        CFRelease(observer);
    }
    if state.input_active() {
        state.emit_unavailable(MacUnavailableReason::Error);
    }
    let cleanup = unsafe { resources.release(context) };
    drop(state);
    let _ = done.send(cleanup);
}

unsafe fn prepare_manager(
    resources: &mut ManagerResources,
    context: *mut c_void,
    state: &mut NativeState,
) -> Result<(), (String, MacUnavailableReason)> {
    let matching = create_device_matching(&state.selection).ok_or(native_error(
        "macos_hid_matching_failed",
        MacUnavailableReason::Error,
    ))?;
    IOHIDManagerSetDeviceMatching(resources.manager, matching);
    CFRelease(matching);
    IOHIDManagerRegisterDeviceRemovalCallback(
        resources.manager,
        Some(device_removed_callback),
        context,
    );
    IOHIDManagerScheduleWithRunLoop(resources.manager, resources.run_loop, kCFRunLoopDefaultMode);
    resources.scheduled = true;
    let open_result = IOHIDManagerOpen(resources.manager, 0);
    if open_result == 0 {
        resources.opened = true;
    } else {
        let access = access_check(IOHIDCheckAccess(IOHID_REQUEST_LISTEN_EVENT));
        if access == AccessCheck::Denied {
            return Err(native_error(
                "macos_input_access_denied",
                access_failure_reason(access),
            ));
        }
        if access == AccessCheck::Unknown {
            let request_accepted = IOHIDRequestAccess(IOHID_REQUEST_LISTEN_EVENT) != 0;
            if !request_accepted {
                return Err(native_error(
                    "macos_input_access_not_granted",
                    access_failure_reason(access),
                ));
            }
            if IOHIDManagerOpen(resources.manager, 0) != 0 {
                let reason = access_failure_reason(access_check(IOHIDCheckAccess(
                    IOHID_REQUEST_LISTEN_EVENT,
                )));
                return Err(native_error("macos_hid_manager_open_failed", reason));
            }
            resources.opened = true;
            state.requires_input_monitoring = true;
        } else {
            return Err(native_error(
                "macos_hid_manager_open_failed",
                MacUnavailableReason::Error,
            ));
        }
    }
    let devices = IOHIDManagerCopyDevices(resources.manager);
    if devices.is_null() {
        return Err(native_error(
            "macos_hid_device_enumeration_failed",
            MacUnavailableReason::Error,
        ));
    }
    let count = CFSetGetCount(devices);
    if count == 0 {
        CFRelease(devices);
        return Err(native_error(
            "macos_selected_hid_device_unavailable",
            MacUnavailableReason::Removed,
        ));
    }
    if count < 0 || count as usize > MAX_MATCHING_DEVICES {
        CFRelease(devices);
        return Err(native_error(
            "macos_hid_device_set_size_invalid",
            MacUnavailableReason::Error,
        ));
    }
    let mut entries = vec![std::ptr::null(); count as usize];
    CFSetGetValues(devices, entries.as_mut_ptr());
    let matching_devices: Vec<_> = entries
        .into_iter()
        .map(|entry| entry.cast_mut())
        .filter(|device| {
            device_identity(*device)
                .map(DeviceIdentity::encode)
                .as_deref()
                == Some(state.selection.input_id.as_str())
                && device_properties_match(*device, &state.selection)
        })
        .collect();
    let [device] = matching_devices.as_slice() else {
        CFRelease(devices);
        return Err(native_error(
            if matching_devices.is_empty() {
                "macos_selected_hid_device_unavailable"
            } else {
                "macos_selected_hid_device_ambiguous"
            },
            if matching_devices.is_empty() {
                MacUnavailableReason::Removed
            } else {
                MacUnavailableReason::Error
            },
        ));
    };
    let device = *device;
    state.selected_device = CFRetain(device).cast_mut();

    let elements = match copy_element_descriptors(device) {
        Ok(elements) => elements,
        Err(error) => {
            CFRelease(devices);
            return Err(native_error(&error, MacUnavailableReason::Error));
        }
    };
    let approved = match approve_descriptor(&state.selection, &state.selection.input_id, &elements)
    {
        Ok(approved) => approved,
        Err(error) => {
            CFRelease(devices);
            return Err(native_error(
                &descriptor_error(error),
                MacUnavailableReason::Error,
            ));
        }
    };
    let Some(element) = elements
        .iter()
        .find(|element| element.cookie == approved.cookie)
    else {
        CFRelease(devices);
        return Err(native_error(
            "macos_hid_element_disappeared",
            MacUnavailableReason::Error,
        ));
    };
    let Some(value_matching) = create_value_matching(element) else {
        CFRelease(devices);
        return Err(native_error(
            "macos_hid_value_matching_failed",
            MacUnavailableReason::Error,
        ));
    };
    IOHIDManagerSetInputValueMatching(resources.manager, value_matching);
    CFRelease(value_matching);
    IOHIDManagerRegisterInputValueCallback(resources.manager, Some(input_value_callback), context);
    state.decoder = Some(InputDecoder::new(
        &state.selection,
        approved,
        state.generation,
    ));
    CFRelease(devices);
    Ok(())
}

unsafe fn create_device_matching(selection: &MacSelection) -> Option<CFMutableDictionaryRef> {
    let identity = DeviceIdentity::decode(&selection.input_id)?;
    let values = [
        (b"VendorID\0".as_slice(), i64::from(selection.vendor_id)),
        (b"ProductID\0".as_slice(), i64::from(selection.product_id)),
        (b"LocationID\0".as_slice(), i64::from(identity.location_id)),
    ];
    create_dictionary(&values)
}
unsafe fn create_value_matching(element: &ElementDescriptor) -> Option<CFMutableDictionaryRef> {
    let values = [
        (b"UsagePage\0".as_slice(), i64::from(element.usage_page)),
        (b"Usage\0".as_slice(), i64::from(element.usage)),
    ];
    create_dictionary(&values)
}
unsafe fn create_dictionary(values: &[(&'static [u8], i64)]) -> Option<CFMutableDictionaryRef> {
    let dictionary = CFDictionaryCreateMutable(
        kCFAllocatorDefault,
        values.len() as CFIndex,
        &kCFTypeDictionaryKeyCallBacks,
        &kCFTypeDictionaryValueCallBacks,
    );
    if dictionary.is_null() {
        return None;
    }
    for (key_text, value) in values {
        let Some(key) = create_hid_key(key_text) else {
            CFRelease(dictionary);
            return None;
        };
        let inserted = set_number(dictionary, key, *value);
        CFRelease(key);
        if !inserted {
            CFRelease(dictionary);
            return None;
        }
    }
    Some(dictionary)
}
unsafe fn create_hid_key(value: &'static [u8]) -> Option<CFStringRef> {
    let key = CFStringCreateWithCString(
        kCFAllocatorDefault,
        value.as_ptr().cast::<c_char>(),
        CF_STRING_ENCODING_UTF8,
    );
    (!key.is_null()).then_some(key)
}
unsafe fn set_number(dictionary: CFMutableDictionaryRef, key: CFStringRef, value: i64) -> bool {
    if key.is_null() {
        return false;
    }
    let number = CFNumberCreate(
        kCFAllocatorDefault,
        CF_NUMBER_SINT64_TYPE,
        (&value as *const i64).cast::<c_void>(),
    );
    if number.is_null() {
        return false;
    }
    CFDictionarySetValue(dictionary, key, number);
    CFRelease(number);
    true
}

unsafe fn device_properties_match(device: IOHIDDeviceRef, selection: &MacSelection) -> bool {
    number_property(device, b"VendorID\0") == Some(i64::from(selection.vendor_id))
        && number_property(device, b"ProductID\0") == Some(i64::from(selection.product_id))
        && number_property(device, b"LocationID\0").and_then(|value| u32::try_from(value).ok())
            == DeviceIdentity::decode(&selection.input_id).map(|identity| identity.location_id)
}
unsafe fn device_identity(device: IOHIDDeviceRef) -> Option<DeviceIdentity> {
    let location_id = u32::try_from(number_property(device, b"LocationID\0")?).ok()?;
    let service = IOHIDDeviceGetService(device);
    if service == 0 {
        return None;
    }
    let mut registry_entry_id = 0;
    if IORegistryEntryGetRegistryEntryID(service, &mut registry_entry_id) != 0 {
        return None;
    }
    Some(DeviceIdentity {
        location_id,
        registry_entry_id,
    })
}
unsafe fn number_property(device: IOHIDDeviceRef, key_text: &'static [u8]) -> Option<i64> {
    let key = create_hid_key(key_text)?;
    let value = IOHIDDeviceGetProperty(device, key);
    let result = if value.is_null() || CFGetTypeID(value) != CFNumberGetTypeID() {
        None
    } else {
        let mut number = 0i64;
        (CFNumberGetValue(
            value,
            CF_NUMBER_SINT64_TYPE,
            (&mut number as *mut i64).cast::<c_void>(),
        ) != 0)
            .then_some(number)
    };
    CFRelease(key);
    result
}

unsafe fn copy_element_descriptors(
    device: IOHIDDeviceRef,
) -> Result<Vec<ElementDescriptor>, String> {
    let elements = IOHIDDeviceCopyMatchingElements(device, std::ptr::null(), 0);
    if elements.is_null() {
        return Err("macos_hid_descriptor_unavailable".into());
    }
    let count = CFArrayGetCount(elements);
    if count <= 0 || count as usize > MAX_DESCRIPTOR_ELEMENTS {
        CFRelease(elements);
        return Err("macos_hid_descriptor_size_invalid".into());
    }
    let mut descriptors = Vec::with_capacity(count as usize);
    for index in 0..count {
        let element = CFArrayGetValueAtIndex(elements, index).cast_mut();
        if element.is_null() {
            CFRelease(elements);
            return Err("macos_hid_descriptor_element_invalid".into());
        }
        let application_collections = match application_collections(element) {
            Ok(collections) => collections,
            Err(error) => {
                CFRelease(elements);
                return Err(error);
            }
        };
        let element_type = IOHIDElementGetType(element);
        descriptors.push(ElementDescriptor {
            cookie: IOHIDElementGetCookie(element),
            usage_page: u16::try_from(IOHIDElementGetUsagePage(element)).unwrap_or(u16::MAX),
            usage: u16::try_from(IOHIDElementGetUsage(element)).unwrap_or(u16::MAX),
            kind: if element_type == IOHID_ELEMENT_TYPE_INPUT_BUTTON {
                ElementKind::Button
            } else {
                ElementKind::Other
            },
            logical_min: IOHIDElementGetLogicalMin(element) as i64,
            logical_max: IOHIDElementGetLogicalMax(element) as i64,
            report_size: IOHIDElementGetReportSize(element),
            report_count: IOHIDElementGetReportCount(element),
            is_array: IOHIDElementIsArray(element) != 0,
            is_relative: IOHIDElementIsRelative(element) != 0,
            application_collections,
        });
    }
    CFRelease(elements);
    Ok(descriptors)
}

unsafe fn application_collections(
    element: IOHIDElementRef,
) -> Result<Vec<super::CollectionIdentity>, String> {
    let mut result = Vec::new();
    let mut parent = IOHIDElementGetParent(element);
    let mut depth = 0;
    while !parent.is_null() && depth < MAX_COLLECTION_DEPTH {
        if IOHIDElementGetType(parent) == IOHID_ELEMENT_TYPE_COLLECTION
            && IOHIDElementGetCollectionType(parent) == IOHID_COLLECTION_TYPE_APPLICATION
        {
            let cookie = IOHIDElementGetCookie(parent);
            let usage_page = u16::try_from(IOHIDElementGetUsagePage(parent))
                .map_err(|_| "macos_hid_collection_usage_invalid".to_string())?;
            let usage = u16::try_from(IOHIDElementGetUsage(parent))
                .map_err(|_| "macos_hid_collection_usage_invalid".to_string())?;
            if cookie == 0 {
                return Err("macos_hid_collection_cookie_invalid".into());
            }
            result.push(super::CollectionIdentity {
                cookie,
                usage_page,
                usage,
            });
        }
        parent = IOHIDElementGetParent(parent);
        depth += 1;
    }
    if !parent.is_null() {
        return Err("macos_hid_collection_depth_exceeded".into());
    }
    Ok(result)
}

fn descriptor_error(error: super::DescriptorError) -> String {
    match error {
        super::DescriptorError::WrongDevice => "macos_hid_device_mismatch",
        super::DescriptorError::WrongCollection => "macos_hid_collection_mismatch",
        super::DescriptorError::MissingApprovedMapping => "macos_hid_element_mapping_unavailable",
        super::DescriptorError::MissingElement => "macos_hid_trigger_element_missing",
        super::DescriptorError::AmbiguousElement => "macos_hid_trigger_element_ambiguous",
        super::DescriptorError::InvalidElement => "macos_hid_trigger_element_invalid",
    }
    .into()
}
unsafe fn state_from_context<'a>(context: *mut c_void) -> &'a mut NativeState {
    &mut *context.cast::<NativeState>()
}
unsafe fn matches_device(state: &NativeState, device: IOHIDDeviceRef) -> bool {
    !device.is_null() && device == state.selected_device
}
unsafe extern "C" fn device_removed_callback(
    context: *mut c_void,
    _result: IOReturn,
    _sender: *mut c_void,
    device: IOHIDDeviceRef,
) {
    let state = state_from_context(context);
    if state.input_active() && matches_device(state, device) {
        state.emit_unavailable(MacUnavailableReason::Removed);
    }
}
unsafe extern "C" fn input_value_callback(
    context: *mut c_void,
    result: IOReturn,
    _sender: *mut c_void,
    value: IOHIDValueRef,
) {
    let state = state_from_context(context);
    if !state.input_active() {
        return;
    }
    if result != 0 {
        state.emit_unavailable(runtime_error_reason());
        return;
    }
    if state.requires_input_monitoring
        && access_check(IOHIDCheckAccess(IOHID_REQUEST_LISTEN_EVENT)) != AccessCheck::Granted
    {
        state.emit_unavailable(MacUnavailableReason::AccessLost);
        return;
    }
    if value.is_null() {
        return;
    }
    let element = IOHIDValueGetElement(value);
    if element.is_null() {
        return;
    }
    let device = IOHIDElementGetDevice(element);
    if !matches_device(state, device) {
        return;
    }
    let Some(expected) = state.decoder.as_ref() else {
        return;
    };
    let Some(approved_usage) = state.selection.approved_element_usage else {
        return;
    };
    let cookie = IOHIDElementGetCookie(element);
    let usage_page = IOHIDElementGetUsagePage(element);
    let usage = IOHIDElementGetUsage(element);
    if IOHIDElementGetType(element) != IOHID_ELEMENT_TYPE_INPUT_BUTTON
        || cookie != expected.element.cookie
        || u16::try_from(usage_page).ok() != Some(approved_usage.usage_page)
        || u16::try_from(usage).ok() != Some(approved_usage.usage)
        || !element_in_selected_collection(element, &state.selection)
    {
        return;
    }
    let input_id = &state.selection.input_id;
    let collection_id = &state.selection.collection_id;
    let decoded = state.decoder.as_mut().and_then(|decoder| {
        decoder.observe(
            input_id,
            collection_id,
            cookie,
            IOHIDValueGetIntegerValue(value) as i64,
        )
    });
    if let Some(event) = decoded.filter(|_| state.input_active()) {
        (state.callback)(event);
    }
}
unsafe fn element_in_selected_collection(
    element: IOHIDElementRef,
    selection: &MacSelection,
) -> bool {
    let Ok(selected) = super::selected_collection(selection) else {
        return false;
    };
    let mut parent = IOHIDElementGetParent(element);
    let mut depth = 0;
    while !parent.is_null() && depth < MAX_COLLECTION_DEPTH {
        if IOHIDElementGetType(parent) == IOHID_ELEMENT_TYPE_COLLECTION
            && IOHIDElementGetCollectionType(parent) == IOHID_COLLECTION_TYPE_APPLICATION
            && IOHIDElementGetCookie(parent) == selected.cookie
            && u16::try_from(IOHIDElementGetUsagePage(parent)).ok() == Some(selected.usage_page)
            && u16::try_from(IOHIDElementGetUsage(parent)).ok() == Some(selected.usage)
        {
            return true;
        }
        parent = IOHIDElementGetParent(parent);
        depth += 1;
    }
    false
}
fn runtime_error_reason() -> MacUnavailableReason {
    let access = access_check(unsafe { IOHIDCheckAccess(IOHID_REQUEST_LISTEN_EVENT) });
    if access == AccessCheck::Granted {
        MacUnavailableReason::Error
    } else {
        MacUnavailableReason::AccessLost
    }
}
fn access_check(value: u32) -> AccessCheck {
    match value {
        IOHID_ACCESS_GRANTED => AccessCheck::Granted,
        IOHID_ACCESS_DENIED => AccessCheck::Denied,
        IOHID_ACCESS_UNKNOWN => AccessCheck::Unknown,
        _ => AccessCheck::Unknown,
    }
}
