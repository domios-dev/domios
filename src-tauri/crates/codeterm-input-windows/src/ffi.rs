use std::ffi::c_void;

pub(super) type Handle = *mut c_void;
pub(super) type Hwnd = *mut c_void;
pub(super) type Hinstance = *mut c_void;
pub(super) type Wparam = usize;
pub(super) type Lparam = isize;
pub(super) type Lresult = isize;
pub(super) type WindowProc = unsafe extern "system" fn(Hwnd, u32, Wparam, Lparam) -> Lresult;

#[rustfmt::skip]
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct RawInputDeviceList {
    pub(super) device: Handle, pub(super) device_type: u32,
}

#[rustfmt::skip]
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct RawInputHeader {
    pub(super) device_type: u32, pub(super) size: u32, pub(super) device: Handle, pub(super) wparam: Wparam,
}

#[rustfmt::skip]
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct RawHidHeader {
    pub(super) report_bytes: u32, pub(super) report_count: u32,
}

#[rustfmt::skip]
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct RawInputDevice {
    pub(super) usage_page: u16, pub(super) usage: u16, pub(super) flags: u32, pub(super) target: Hwnd,
}

#[rustfmt::skip]
#[repr(C)]
pub(super) struct Msg {
    pub(super) window: Hwnd, pub(super) message: u32, pub(super) wparam: Wparam, pub(super) lparam: Lparam,
    pub(super) time: u32, pub(super) point_x: i32, pub(super) point_y: i32, pub(super) private: u32,
}

#[rustfmt::skip]
#[repr(C)]
pub(super) struct WndClassW {
    pub(super) style: u32, pub(super) procedure: Option<WindowProc>, pub(super) class_extra: i32, pub(super) window_extra: i32,
    pub(super) instance: Hinstance, pub(super) icon: Handle, pub(super) cursor: Handle, pub(super) background: Handle,
    pub(super) menu_name: *const u16, pub(super) class_name: *const u16,
}

#[rustfmt::skip]
#[repr(C)]
pub(super) struct CreateStructW {
    pub(super) parameter: *mut c_void, pub(super) instance: Hinstance, pub(super) menu: Handle, pub(super) parent: Hwnd,
    pub(super) cy: i32, pub(super) cx: i32, pub(super) y: i32, pub(super) x: i32, pub(super) style: i32,
    pub(super) name: *const u16, pub(super) class: *const u16, pub(super) extended_style: u32,
}

#[rustfmt::skip]
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct HidpCaps {
    pub(super) usage: u16, pub(super) usage_page: u16, pub(super) input_report_bytes: u16, pub(super) output_report_bytes: u16,
    pub(super) feature_report_bytes: u16, pub(super) reserved: [u16; 17],
    pub(super) link_collections: u16, pub(super) input_button_caps: u16, pub(super) input_value_caps: u16,
    pub(super) input_data_indices: u16, pub(super) output_button_caps: u16, pub(super) output_value_caps: u16,
    pub(super) output_data_indices: u16, pub(super) feature_button_caps: u16, pub(super) feature_value_caps: u16,
    pub(super) feature_data_indices: u16,
}

#[rustfmt::skip]
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct HidpUsageRange {
    pub(super) usage_min: u16, pub(super) usage_max: u16, pub(super) string_min: u16, pub(super) string_max: u16,
    pub(super) designator_min: u16, pub(super) designator_max: u16, pub(super) data_index_min: u16, pub(super) data_index_max: u16,
}

#[rustfmt::skip]
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct HidpUsageSingle {
    pub(super) usage: u16, pub(super) reserved_1: u16, pub(super) string_index: u16, pub(super) reserved_2: u16,
    pub(super) designator_index: u16, pub(super) reserved_3: u16, pub(super) data_index: u16, pub(super) reserved_4: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) union HidpButtonCapsData {
    pub(super) range: HidpUsageRange,
    pub(super) not_range: HidpUsageSingle,
}

#[rustfmt::skip]
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct HidpButtonCaps {
    pub(super) usage_page: u16, pub(super) report_id: u8, pub(super) is_alias: u8, pub(super) bit_field: u16,
    pub(super) link_collection: u16, pub(super) link_usage: u16, pub(super) link_usage_page: u16,
    pub(super) is_range: u8, pub(super) is_string_range: u8, pub(super) is_designator_range: u8, pub(super) is_absolute: u8,
    pub(super) report_count: u16, pub(super) reserved_2: u16, pub(super) reserved: [u32; 9], pub(super) data: HidpButtonCapsData,
}

pub(super) const HIDP_INPUT: i32 = 0;
pub(super) const HWND_MESSAGE: Hwnd = -3isize as Hwnd;

#[link(name = "user32")]
extern "system" {
    pub(super) fn GetRawInputDeviceList(
        devices: *mut RawInputDeviceList,
        count: *mut u32,
        size: u32,
    ) -> u32;
    pub(super) fn GetRawInputDeviceInfoW(
        device: Handle,
        command: u32,
        data: *mut c_void,
        size: *mut u32,
    ) -> u32;
    pub(super) fn GetRawInputData(
        raw_input: Handle,
        command: u32,
        data: *mut c_void,
        size: *mut u32,
        header_size: u32,
    ) -> u32;
    pub(super) fn RegisterRawInputDevices(
        devices: *const RawInputDevice,
        count: u32,
        size: u32,
    ) -> i32;
    pub(super) fn GetRegisteredRawInputDevices(
        devices: *mut RawInputDevice,
        count: *mut u32,
        size: u32,
    ) -> u32;
    pub(super) fn RegisterClassW(window_class: *const WndClassW) -> u16;
    pub(super) fn CreateWindowExW(
        extended_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Hwnd,
        menu: Handle,
        instance: Hinstance,
        parameter: *mut c_void,
    ) -> Hwnd;
    pub(super) fn DestroyWindow(window: Hwnd) -> i32;
    pub(super) fn GetMessageW(message: *mut Msg, window: Hwnd, min: u32, max: u32) -> i32;
    pub(super) fn TranslateMessage(message: *const Msg) -> i32;
    pub(super) fn DispatchMessageW(message: *const Msg) -> Lresult;
    pub(super) fn DefWindowProcW(
        window: Hwnd,
        message: u32,
        wparam: Wparam,
        lparam: Lparam,
    ) -> Lresult;
    pub(super) fn SetWindowLongPtrW(window: Hwnd, index: i32, value: isize) -> isize;
    pub(super) fn GetWindowLongPtrW(window: Hwnd, index: i32) -> isize;
    pub(super) fn PeekMessageW(
        message: *mut Msg,
        window: Hwnd,
        min: u32,
        max: u32,
        remove: u32,
    ) -> i32;
    pub(super) fn PostThreadMessageW(
        thread_id: u32,
        message: u32,
        wparam: Wparam,
        lparam: Lparam,
    ) -> i32;
    pub(super) fn PostQuitMessage(exit_code: i32);
}

#[link(name = "kernel32")]
extern "system" {
    pub(super) fn GetCurrentThreadId() -> u32;
    pub(super) fn GetModuleHandleW(name: *const u16) -> Hinstance;
    pub(super) fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
}

#[link(name = "hid")]
extern "system" {
    pub(super) fn HidP_GetCaps(preparsed: *mut c_void, caps: *mut HidpCaps) -> i32;
    pub(super) fn HidP_GetButtonCaps(
        report_type: i32,
        caps: *mut HidpButtonCaps,
        caps_length: *mut u16,
        preparsed: *mut c_void,
    ) -> i32;
    pub(super) fn HidP_GetUsages(
        report_type: i32,
        usage_page: u16,
        link_collection: u16,
        usages: *mut u16,
        usage_length: *mut u32,
        preparsed: *mut c_void,
        report: *mut i8,
        report_length: u32,
    ) -> i32;
}
