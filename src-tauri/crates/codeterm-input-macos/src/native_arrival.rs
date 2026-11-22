use super::native::{
    create_dictionary, device_identity, kCFAllocatorDefault, kCFRunLoopDefaultMode,
    number_property, request_run_loop_stop, run_loop_started, CFRelease, CFRetain,
    CFRunLoopAddObserver, CFRunLoopGetCurrent, CFRunLoopObserverContext, CFRunLoopObserverCreate,
    CFRunLoopObserverInvalidate, CFRunLoopRef, CFRunLoopRun, CFSetGetCount, CFSetGetValues,
    CFTypeRef, CoreFoundationRunLoop, IOHIDDeviceRef, IOHIDManagerClose, IOHIDManagerCopyDevices,
    IOHIDManagerCreate, IOHIDManagerOpen, IOHIDManagerRef,
    IOHIDManagerRegisterDeviceMatchingCallback, IOHIDManagerRegisterDeviceRemovalCallback,
    IOHIDManagerScheduleWithRunLoop, IOHIDManagerSetDeviceMatching,
    IOHIDManagerUnscheduleFromRunLoop, IOReturn, MacSetupFailure, MacTeardown, RunLoopStart,
    CF_RUN_LOOP_ENTRY, MAX_MATCHING_DEVICES, SETUP_ACK_TIMEOUT,
};
use super::{
    abandon_ready_run_loop, arrival_filter_valid, ArrivalTracker, DeviceIdentity, MacArrivalEvent,
    MacArrivalFilter, RunLoopHandle,
};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::Arc;
use std::thread;

pub struct MacArrivalWatcher {
    active: Arc<AtomicBool>,
    run_loop: Option<RunLoopHandle>,
    completion: Option<Receiver<Result<(), String>>>,
}

impl MacArrivalWatcher {
    pub fn request_stop(&mut self) -> Option<MacTeardown> {
        self.active.store(false, Ordering::Release);
        if let Some(run_loop) = self.run_loop.as_mut() {
            run_loop.stop();
        }
        self.completion
            .take()
            .map(|completion| MacTeardown::new_pending(completion, self.run_loop.take()))
    }
}

impl Drop for MacArrivalWatcher {
    fn drop(&mut self) {
        if self.completion.is_some() {
            self.active.store(false, Ordering::Release);
            if let Some(run_loop) = self.run_loop.as_mut() {
                run_loop.stop();
            }
            self.completion.take();
        }
    }
}

struct ArrivalState {
    filter: MacArrivalFilter,
    tracker: ArrivalTracker,
    callback: Arc<dyn Fn(MacArrivalEvent) + Send + Sync>,
    active: Arc<AtomicBool>,
    run_loop: CFRunLoopRef,
    run_loop_stop: Arc<AtomicBool>,
}

struct ArrivalResources {
    manager: IOHIDManagerRef,
    run_loop: CFRunLoopRef,
    scheduled: bool,
    opened: bool,
}

impl ArrivalResources {
    unsafe fn release(&mut self, context: *mut c_void) -> Result<(), String> {
        if self.manager.is_null() {
            return Ok(());
        }
        IOHIDManagerRegisterDeviceMatchingCallback(self.manager, None, context);
        IOHIDManagerRegisterDeviceRemovalCallback(self.manager, None, context);
        let close_failed = self.opened && IOHIDManagerClose(self.manager, 0) != 0;
        if self.scheduled {
            IOHIDManagerUnscheduleFromRunLoop(self.manager, self.run_loop, kCFRunLoopDefaultMode);
        }
        CFRelease(self.manager);
        self.manager = std::ptr::null_mut();
        if close_failed {
            Err("macos_hid_arrival_manager_close_failed".into())
        } else {
            Ok(())
        }
    }
}

pub(crate) fn watch_arrivals(
    filter: MacArrivalFilter,
    callback: Arc<dyn Fn(MacArrivalEvent) + Send + Sync>,
) -> Result<MacArrivalWatcher, MacSetupFailure> {
    if !arrival_filter_valid(&filter) {
        return Err(MacSetupFailure {
            reason: "macos_arrival_filter_invalid".into(),
            teardown: None,
        });
    }
    let active = Arc::new(AtomicBool::new(true));
    let worker_active = active.clone();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("local-recording-macos-arrival".into())
        .spawn(move || {
            run_arrival_session(filter, callback, worker_active, ready_tx, done_tx);
        })
        .map_err(|_| MacSetupFailure {
            reason: "macos_arrival_thread_unavailable".into(),
            teardown: None,
        })?;

    let run_loop = match ready_rx.recv_timeout(SETUP_ACK_TIMEOUT) {
        Ok(Ok(run_loop)) => run_loop,
        Ok(Err(reason)) => {
            return Err(MacSetupFailure {
                reason,
                teardown: Some(MacTeardown::new_pending(done_rx, None)),
            });
        }
        Err(RecvTimeoutError::Timeout) => {
            active.store(false, Ordering::Release);
            return Err(MacSetupFailure {
                reason: "macos_arrival_setup_timeout".into(),
                teardown: Some(MacTeardown::new_pending(done_rx, None)),
            });
        }
        Err(RecvTimeoutError::Disconnected) => {
            active.store(false, Ordering::Release);
            return Err(MacSetupFailure {
                reason: "macos_arrival_start_unacknowledged".into(),
                teardown: Some(MacTeardown::new_pending(done_rx, None)),
            });
        }
    };
    Ok(MacArrivalWatcher {
        active,
        run_loop: Some(run_loop),
        completion: Some(done_rx),
    })
}

fn fail_arrival_setup(
    state: Box<ArrivalState>,
    mut resources: ArrivalResources,
    context: *mut c_void,
    error: String,
    ready: SyncSender<Result<RunLoopHandle, String>>,
    done: SyncSender<Result<(), String>>,
) {
    state.active.store(false, Ordering::Release);
    request_run_loop_stop(state.run_loop, &state.run_loop_stop);
    let _ = ready.send(Err(error));
    let cleanup = unsafe { resources.release(context) };
    drop(state);
    let _ = done.send(cleanup);
}

fn run_arrival_session(
    filter: MacArrivalFilter,
    callback: Arc<dyn Fn(MacArrivalEvent) + Send + Sync>,
    active: Arc<AtomicBool>,
    ready: SyncSender<Result<RunLoopHandle, String>>,
    done: SyncSender<Result<(), String>>,
) {
    let run_loop = unsafe { CFRunLoopGetCurrent() };
    if run_loop.is_null() {
        let _ = ready.send(Err("macos_run_loop_unavailable".into()));
        let _ = done.send(Ok(()));
        return;
    }
    let manager = unsafe { IOHIDManagerCreate(kCFAllocatorDefault, 0) };
    if manager.is_null() {
        let _ = ready.send(Err("macos_hid_manager_unavailable".into()));
        let _ = done.send(Ok(()));
        return;
    }
    let mut resources = ArrivalResources {
        manager,
        run_loop,
        scheduled: false,
        opened: false,
    };
    let mut state = Box::new(ArrivalState {
        filter,
        tracker: ArrivalTracker::new(MAX_MATCHING_DEVICES),
        callback,
        active,
        run_loop,
        run_loop_stop: Arc::new(AtomicBool::new(false)),
    });
    let context = (&mut *state as *mut ArrivalState).cast::<c_void>();
    if let Err(error) = unsafe { prepare_arrival_manager(&mut resources, context, &mut state) } {
        return fail_arrival_setup(state, resources, context, error, ready, done);
    }
    if !state.active.load(Ordering::Acquire) {
        return fail_arrival_setup(
            state,
            resources,
            context,
            "macos_arrival_setup_cancelled".into(),
            ready,
            done,
        );
    }
    let retained_run_loop = unsafe { CFRetain(run_loop as CFTypeRef) as CFRunLoopRef };
    if retained_run_loop.is_null() {
        return fail_arrival_setup(
            state,
            resources,
            context,
            "macos_run_loop_retain_failed".into(),
            ready,
            done,
        );
    }
    let mut start = RunLoopStart {
        ready: Some(ready),
        run_loop: Some(RunLoopHandle::new(CoreFoundationRunLoop {
            run_loop: retained_run_loop,
            stop_requested: state.run_loop_stop.clone(),
        })),
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
        drop(start.run_loop.take());
        return fail_arrival_setup(
            state,
            resources,
            context,
            "macos_run_loop_observer_unavailable".into(),
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
    if let Some(ready) = start.ready.take() {
        abandon_ready_run_loop(
            ready,
            "macos_arrival_start_unacknowledged",
            start.run_loop.take(),
        );
    }
    let cleanup = unsafe { resources.release(context) };
    drop(state);
    let _ = done.send(cleanup);
}

unsafe fn prepare_arrival_manager(
    resources: &mut ArrivalResources,
    context: *mut c_void,
    state: &mut ArrivalState,
) -> Result<(), String> {
    let values = [
        (b"VendorID\0".as_slice(), i64::from(state.filter.vendor_id)),
        (
            b"ProductID\0".as_slice(),
            i64::from(state.filter.product_id),
        ),
        (
            b"PrimaryUsagePage\0".as_slice(),
            i64::from(state.filter.collection_usage_page),
        ),
        (
            b"PrimaryUsage\0".as_slice(),
            i64::from(state.filter.collection_usage),
        ),
    ];
    let matching = create_dictionary(&values).ok_or("macos_hid_matching_failed")?;
    IOHIDManagerSetDeviceMatching(resources.manager, matching);
    CFRelease(matching);
    IOHIDManagerScheduleWithRunLoop(resources.manager, resources.run_loop, kCFRunLoopDefaultMode);
    resources.scheduled = true;
    if IOHIDManagerOpen(resources.manager, 0) != 0 {
        return Err("macos_hid_arrival_manager_open_failed".into());
    }
    resources.opened = true;
    seed_existing_devices(resources.manager, &mut state.tracker);
    IOHIDManagerRegisterDeviceMatchingCallback(
        resources.manager,
        Some(device_matching_callback),
        context,
    );
    IOHIDManagerRegisterDeviceRemovalCallback(
        resources.manager,
        Some(device_removal_callback),
        context,
    );
    Ok(())
}

unsafe fn seed_existing_devices(manager: IOHIDManagerRef, tracker: &mut ArrivalTracker) {
    let devices = IOHIDManagerCopyDevices(manager);
    if devices.is_null() {
        return;
    }
    let count = CFSetGetCount(devices);
    if count > 0 && count as usize <= MAX_MATCHING_DEVICES {
        let mut entries = vec![std::ptr::null(); count as usize];
        CFSetGetValues(devices, entries.as_mut_ptr());
        for entry in entries {
            if let Some(identity) = device_identity(entry.cast_mut()) {
                tracker.seed(DeviceIdentity::encode(identity));
            }
        }
    }
    CFRelease(devices);
}

unsafe fn device_matches_filter(device: IOHIDDeviceRef, filter: &MacArrivalFilter) -> bool {
    !device.is_null()
        && number_property(device, b"VendorID\0") == Some(i64::from(filter.vendor_id))
        && number_property(device, b"ProductID\0") == Some(i64::from(filter.product_id))
}

unsafe extern "C" fn device_matching_callback(
    context: *mut c_void,
    _result: IOReturn,
    _sender: *mut c_void,
    device: IOHIDDeviceRef,
) {
    let state = &mut *context.cast::<ArrivalState>();
    if !state.active.load(Ordering::Acquire) || !device_matches_filter(device, &state.filter) {
        return;
    }
    let Some(identity) = device_identity(device) else {
        return;
    };
    let input_id = identity.encode();
    if state.tracker.observe_arrival(input_id.clone()) {
        let callback = &state.callback;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            callback(MacArrivalEvent { input_id });
        }));
    }
}

unsafe extern "C" fn device_removal_callback(
    context: *mut c_void,
    _result: IOReturn,
    _sender: *mut c_void,
    device: IOHIDDeviceRef,
) {
    let state = &mut *context.cast::<ArrivalState>();
    if device.is_null() {
        return;
    }
    if let Some(identity) = device_identity(device) {
        state.tracker.observe_removal(&identity.encode());
    }
}
