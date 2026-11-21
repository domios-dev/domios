use super::{
    complete_teardown, ffi::*, has_raw_input_registration, last_error_code, raw_device_name,
    raw_input_devices, setup_entry_result, supports_trigger, unavailable_reason_for_win32,
    wide_null, ListenerStartError, SelectedInputIdentity, SetupFailure, TeardownError,
    ThreadControl, UnavailableReason, WindowsInputTeardown, GIDC_ARRIVAL, GIDC_REMOVAL,
    GWLP_USERDATA, LISTENER_CLAIMED, MAX_INPUT_DEVICES, PM_NOREMOVE, RIDEV_DEVNOTIFY,
    RIDEV_INPUTSINK, RIDEV_REMOVE, RIM_TYPE_HID, WM_INPUT_DEVICE_CHANGE, WM_NCCREATE,
};
use codeterm_local_recording::LocalRecordingTrigger;
use std::collections::BTreeSet;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::thread::{self, JoinHandle};

static ARRIVAL_WINDOW_CLASS: OnceLock<Result<(), u32>> = OnceLock::new();
static ARRIVAL_WINDOW_CLASS_NAME: OnceLock<Vec<u16>> = OnceLock::new();

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsArrivalEvent {
    pub input_id: String,
}

pub struct WindowsArrivalPendingWatch {
    control: Arc<ThreadControl>,
    startup: Option<Receiver<Result<(), SetupFailure>>>,
    completion: Option<Receiver<Result<(), TeardownError>>>,
    thread: Option<JoinHandle<()>>,
    abandoned: bool,
}

pub struct WindowsArrivalWatcher {
    control: Arc<ThreadControl>,
    completion: Option<Receiver<Result<(), TeardownError>>>,
    thread: Option<JoinHandle<()>>,
    stop_requested: bool,
}

pub struct WindowsArrivalSetupFailure {
    pub reason: SetupFailure,
    pub teardown: WindowsInputTeardown,
}

pub(crate) struct ArrivalHandleTracker {
    known: BTreeSet<usize>,
    max_entries: usize,
}

impl ArrivalHandleTracker {
    pub(crate) fn new(max_entries: usize) -> Self {
        Self {
            known: BTreeSet::new(),
            max_entries,
        }
    }

    pub(crate) fn seed(&mut self, handle: usize) {
        if handle != 0 && self.known.len() < self.max_entries {
            self.known.insert(handle);
        }
    }

    pub(crate) fn observe_arrival(&mut self, handle: usize) -> bool {
        if handle == 0 || self.known.contains(&handle) {
            return false;
        }
        if self.known.len() < self.max_entries {
            self.known.insert(handle);
        }
        true
    }

    pub(crate) fn observe_removal(&mut self, handle: usize) {
        self.known.remove(&handle);
    }
}

pub(crate) fn arrival_matches_trigger(trigger: &LocalRecordingTrigger, path: &str) -> bool {
    let Some(button) = trigger.button.as_ref() else {
        return false;
    };
    let identity = SelectedInputIdentity {
        input_id: path.to_string(),
        collection_id: button.collection_id.clone(),
        trigger: trigger.clone(),
    };
    identity.is_supported_collection()
}

impl WindowsArrivalWatcher {
    pub fn start_pending(
        trigger: LocalRecordingTrigger,
        callback: impl Fn(WindowsArrivalEvent) + Send + Sync + 'static,
    ) -> Result<WindowsArrivalPendingWatch, String> {
        if !supports_trigger(&trigger) {
            return Err("local_input_descriptor_unsupported".into());
        }
        LISTENER_CLAIMED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "a Raw Input listener is already active".to_string())?;
        let control = Arc::new(ThreadControl::default());
        let (startup_tx, startup_rx) = mpsc::channel();
        let (completion_tx, completion_rx) = mpsc::channel();
        let callback: Arc<dyn Fn(WindowsArrivalEvent) + Send + Sync> = Arc::new(callback);
        let thread_control = Arc::clone(&control);
        let thread = thread::Builder::new()
            .name("local-recording-arrival".into())
            .spawn(move || {
                run_arrival_listener(
                    thread_control,
                    trigger,
                    callback,
                    startup_tx,
                    completion_tx,
                )
            })
            .map_err(|error| {
                LISTENER_CLAIMED.store(false, Ordering::Release);
                format!("could not start local input arrival thread: {error}")
            })?;
        Ok(WindowsArrivalPendingWatch {
            control,
            startup: Some(startup_rx),
            completion: Some(completion_rx),
            thread: Some(thread),
            abandoned: false,
        })
    }

    pub fn request_stop(mut self) -> WindowsInputTeardown {
        self.stop_requested = true;
        WindowsInputTeardown::stopping(&self.control, self.completion.take(), self.thread.take())
    }
}

impl Drop for WindowsArrivalWatcher {
    fn drop(&mut self) {
        if !self.stop_requested {
            self.control.request_stop();
        }
    }
}

impl WindowsArrivalPendingWatch {
    pub fn wait_until_ready(
        mut self,
    ) -> Result<WindowsArrivalWatcher, WindowsArrivalSetupFailure> {
        let result = self
            .startup
            .as_ref()
            .expect("startup receiver")
            .recv_timeout(super::lifecycle::SETUP_DEADLINE);
        match result {
            Ok(Ok(())) => {
                self.startup.take();
                self.abandoned = true;
                Ok(WindowsArrivalWatcher {
                    control: Arc::clone(&self.control),
                    completion: self.completion.take(),
                    thread: self.thread.take(),
                    stop_requested: false,
                })
            }
            Ok(Err(reason)) => Err(self.fail_setup(reason)),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(self.fail_setup(SetupFailure::Timeout)),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(self.fail_setup(SetupFailure::CompletionLost))
            }
        }
    }

    fn fail_setup(self, reason: SetupFailure) -> WindowsArrivalSetupFailure {
        WindowsArrivalSetupFailure {
            reason,
            teardown: self.into_teardown(),
        }
    }

    fn into_teardown(mut self) -> WindowsInputTeardown {
        self.startup.take();
        self.abandoned = true;
        WindowsInputTeardown::stopping(&self.control, self.completion.take(), self.thread.take())
    }
}

impl Drop for WindowsArrivalPendingWatch {
    fn drop(&mut self) {
        if !self.abandoned {
            self.control.request_stop();
        }
    }
}

struct ArrivalContext {
    trigger: LocalRecordingTrigger,
    tracker: ArrivalHandleTracker,
    callback: Arc<dyn Fn(WindowsArrivalEvent) + Send + Sync>,
}

struct NativeArrivalListener {
    window: Hwnd,
    registered: bool,
    registration_usage_page: u16,
    registration_usage: u16,
    release_attempted: bool,
    context: Option<Box<ArrivalContext>>,
}

impl NativeArrivalListener {
    fn new() -> Self {
        Self {
            window: null_mut(),
            registered: false,
            registration_usage_page: 0,
            registration_usage: 0,
            release_attempted: false,
            context: None,
        }
    }

    fn start(
        &mut self,
        trigger: LocalRecordingTrigger,
        callback: Arc<dyn Fn(WindowsArrivalEvent) + Send + Sync>,
        stop_requested: &AtomicBool,
    ) -> Result<(), ListenerStartError> {
        if stop_requested.load(Ordering::Acquire) {
            return Err(ListenerStartError::Stopped);
        }
        if !supports_trigger(&trigger) {
            return Err(ListenerStartError::Unavailable(UnavailableReason::Error));
        }
        register_arrival_window_class()
            .map_err(unavailable_reason_for_win32)
            .map_err(ListenerStartError::Unavailable)?;
        let mut tracker = ArrivalHandleTracker::new(MAX_INPUT_DEVICES);
        if let Ok(devices) = raw_input_devices() {
            for device in devices {
                if device.device_type != RIM_TYPE_HID || device.device.is_null() {
                    continue;
                }
                if let Ok(Some(path)) = raw_device_name(device.device) {
                    if arrival_matches_trigger(&trigger, &path) {
                        tracker.seed(device.device as usize);
                    }
                }
            }
        }
        if stop_requested.load(Ordering::Acquire) {
            return Err(ListenerStartError::Stopped);
        }
        self.registration_usage_page = trigger.usage_page;
        self.registration_usage = trigger.usage;
        let mut context = Box::new(ArrivalContext {
            trigger,
            tracker,
            callback,
        });
        let class_name = wide_null("SelectedLocalRawInputArrivalWindow");
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
                (&mut *context as *mut ArrivalContext).cast(),
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
        if has_raw_input_registration(self.registration_usage_page, self.registration_usage)
            .map_err(ListenerStartError::Unavailable)?
        {
            return Err(ListenerStartError::Unavailable(UnavailableReason::Error));
        }
        let registration = RawInputDevice {
            usage_page: self.registration_usage_page,
            usage: self.registration_usage,
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
                usage_page: self.registration_usage_page,
                usage: self.registration_usage,
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

impl Drop for NativeArrivalListener {
    fn drop(&mut self) {
        if !self.release_attempted {
            let _ = self.release();
        }
        if !self.window.is_null() {
            std::mem::forget(self.context.take());
        }
    }
}

fn run_arrival_listener(
    control: Arc<ThreadControl>,
    trigger: LocalRecordingTrigger,
    callback: Arc<dyn Fn(WindowsArrivalEvent) + Send + Sync>,
    startup: Sender<Result<(), SetupFailure>>,
    completion: Sender<Result<(), TeardownError>>,
) {
    let mut listener = NativeArrivalListener::new();
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
        listener.start(trigger, callback, &control.stop_requested)
    };
    match start {
        Err(ListenerStartError::Stopped) => {
            let _ = startup.send(Err(SetupFailure::Stopped));
        }
        Err(ListenerStartError::Unavailable(reason)) => {
            let _ = startup.send(Err(SetupFailure::Unavailable(reason)));
        }
        Ok(()) => {
            let _ = listener.pump(|| {
                let _ = startup.send(setup_entry_result(
                    control.stop_requested.load(Ordering::Acquire),
                ));
            });
        }
    }
    let released = listener.release();
    control.thread_id.store(0, Ordering::Release);
    if released.is_ok() {
        LISTENER_CLAIMED.store(false, Ordering::Release);
    }
    complete_teardown(|| released, completion);
}

fn register_arrival_window_class() -> Result<(), u32> {
    *ARRIVAL_WINDOW_CLASS.get_or_init(|| unsafe {
        let class_name = ARRIVAL_WINDOW_CLASS_NAME
            .get_or_init(|| wide_null("SelectedLocalRawInputArrivalWindow"));
        let instance = GetModuleHandleW(null());
        if instance.is_null() {
            return Err(last_error_code());
        }
        let window_class = WndClassW {
            style: 0,
            procedure: Some(arrival_window_proc),
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

fn emit_arrival(
    callback: &Arc<dyn Fn(WindowsArrivalEvent) + Send + Sync>,
    event: WindowsArrivalEvent,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(event)));
}

unsafe extern "system" fn arrival_window_proc(
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
    let context = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut ArrivalContext;
    if context.is_null() {
        return DefWindowProcW(window, message, wparam, lparam);
    }
    match message {
        WM_INPUT_DEVICE_CHANGE if wparam == GIDC_REMOVAL => {
            (*context).tracker.observe_removal(lparam as usize);
        }
        WM_INPUT_DEVICE_CHANGE if wparam == GIDC_ARRIVAL => {
            let device = lparam as Handle;
            if !device.is_null() && (*context).tracker.observe_arrival(device as usize) {
                if let Ok(Some(path)) = raw_device_name(device) {
                    if arrival_matches_trigger(&(*context).trigger, &path) {
                        emit_arrival(&(*context).callback, WindowsArrivalEvent { input_id: path });
                    }
                }
            }
        }
        _ => {}
    }
    DefWindowProcW(window, message, wparam, lparam)
}
