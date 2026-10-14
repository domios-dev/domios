use super::{ffi::*, raw_device_name, raw_device_preparsed_data, raw_input_devices};
use codeterm_local_recording::ReceiverStatusQuery;
use std::ptr::{null, null_mut};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, ERROR_IO_PENDING, HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{
        CreateFileW, ReadFile, WriteFile, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING,
    },
    System::{
        Threading::{CreateEventW, WaitForSingleObject},
        IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
    },
};

static QUERY: parking_lot::Mutex<()> = parking_lot::const_mutex(());

pub(crate) fn same_receiver(selected: &str, candidate: &str) -> bool {
    let selected = selected.to_ascii_uppercase();
    let candidate = candidate.to_ascii_uppercase();
    let a: Vec<_> = selected.split('#').collect();
    let b: Vec<_> = candidate.split('#').collect();
    a.len() == 4
        && b.len() == 4
        && a[0] == b[0]
        && a[3] == b[3]
        && a[1].rsplit_once("&COL").is_some()
        && b[1].rsplit_once("&COL").is_some()
        && a[1].rsplit_once("&COL").map(|v| v.0) == b[1].rsplit_once("&COL").map(|v| v.0)
        && a[2].rsplit_once('&').is_some()
        && a[2].rsplit_once('&').map(|v| v.0) == b[2].rsplit_once('&').map(|v| v.0)
}

fn selected_path(query: &ReceiverStatusQuery, selected: &str) -> Result<String, String> {
    let mut found = None;
    for device in raw_input_devices().map_err(|_| "receiver_status_inventory_failed")? {
        if device.device_type != 2 {
            continue;
        }
        let Some(path) =
            raw_device_name(device.device).map_err(|_| "receiver_status_inventory_failed")?
        else {
            continue;
        };
        if !same_receiver(selected, &path) {
            continue;
        }
        let data = raw_device_preparsed_data(device.device)
            .map_err(|_| "receiver_status_descriptor_failed")?;
        let mut caps = HidpCaps::default();
        let mut values = [0u8; 512];
        let mut count = 1u16;
        unsafe {
            if HidP_GetCaps(data.as_ptr(), &mut caps) != 0x00110000 {
                return Err("receiver_status_descriptor_unavailable".into());
            }
            if !declared_collection_usage(query, &caps) {
                continue;
            }
            if (
                caps.usage_page,
                caps.usage,
                caps.input_report_bytes,
                caps.output_report_bytes,
            ) != (
                query.usage_page,
                query.usage,
                query.request.len() as u16,
                query.request.len() as u16,
            ) || HidP_GetValueCaps(0, values.as_mut_ptr().cast(), &mut count, data.as_ptr())
                != 0x00110000
                || count != 1
                || values[2] != query.report_id
            {
                return Err("receiver_status_descriptor_unavailable".into());
            }
        }
        if found.replace(path).is_some() {
            return Err("receiver_status_receiver_ambiguous".into());
        }
    }
    found.ok_or_else(|| "receiver_status_receiver_unavailable".into())
}

fn declared_collection_usage(query: &ReceiverStatusQuery, caps: &HidpCaps) -> bool {
    (caps.usage_page, caps.usage) == (query.usage_page, query.usage)
}

struct File(HANDLE);
impl Drop for File {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

struct Operation {
    file: HANDLE,
    overlapped: OVERLAPPED,
    bytes: [u8; 64],
    pending: bool,
}
impl Operation {
    fn start(
        file: HANDLE,
        request: Option<&[u8]>,
        report_bytes: usize,
    ) -> Result<Box<Self>, String> {
        let event = unsafe { CreateEventW(null(), 1, 0, null()) };
        if event.is_null() {
            return Err("receiver_status_event_failed".into());
        }
        let mut operation = Box::new(Self {
            file,
            overlapped: unsafe { std::mem::zeroed() },
            bytes: [0; 64],
            pending: false,
        });
        operation.overlapped.hEvent = event;
        if let Some(request) = request {
            operation.bytes[..request.len()].copy_from_slice(request);
        }
        let mut transferred = 0;
        let started = unsafe {
            if request.is_some() {
                WriteFile(
                    file,
                    operation.bytes.as_ptr(),
                    report_bytes as u32,
                    &mut transferred,
                    &mut operation.overlapped,
                )
            } else {
                ReadFile(
                    file,
                    operation.bytes.as_mut_ptr(),
                    report_bytes as u32,
                    &mut transferred,
                    &mut operation.overlapped,
                )
            }
        };
        if started == 0 && unsafe { GetLastError() } != ERROR_IO_PENDING {
            return Err("receiver_status_io_failed".into());
        }
        operation.pending = true;
        Ok(operation)
    }
    fn finish(&mut self, deadline: u32, report_bytes: usize) -> Result<(), String> {
        if unsafe { WaitForSingleObject(self.overlapped.hEvent, deadline) } != 0 {
            return Err("receiver_status_response_deadline".into());
        }
        let mut transferred = 0;
        if unsafe { GetOverlappedResult(self.file, &self.overlapped, &mut transferred, 0) } == 0 {
            return Err("receiver_status_io_failed".into());
        }
        self.pending = false;
        if transferred as usize != report_bytes {
            return Err("receiver_status_incomplete_frame".into());
        }
        Ok(())
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        unsafe {
            if self.pending {
                CancelIoEx(self.file, &self.overlapped);
                let mut transferred = 0;
                GetOverlappedResult(self.file, &self.overlapped, &mut transferred, 1);
            }
            CloseHandle(self.overlapped.hEvent);
        }
    }
}

pub fn read(query: &ReceiverStatusQuery, selected: &str) -> Result<Vec<u8>, String> {
    if !query.valid() {
        return Err("receiver_status_invalid_declaration".into());
    }
    let _query = QUERY.try_lock().ok_or("receiver_status_query_busy")?;
    let path = selected_path(query, selected)?;
    let name: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            0xc0000000,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err("receiver_status_open_failed".into());
    }
    let file = File(handle);
    let size = query.request.len();
    let mut response = Operation::start(file.0, None, size)?;
    let mut request = Operation::start(file.0, Some(&query.request), size)?;
    request.finish(2000, size)?;
    response.finish(3000, size)?;
    Ok(response.bytes[..size].to_vec())
}

#[link(name = "hid")]
extern "system" {
    fn HidP_GetValueCaps(
        report_type: i32,
        caps: *mut std::ffi::c_void,
        count: *mut u16,
        preparsed: *mut std::ffi::c_void,
    ) -> i32;
}

#[cfg(test)]
mod tests {
    use super::same_receiver;
    #[test]
    fn receiver_status_collection_must_belong_to_the_exact_selected_receiver() {
        let selected = r"\\?\HID#VID_3547&PID_0008&MI_00&Col01#8&2aa4ce89&0&0000#{guid}";
        let candidate = selected.replace("Col01", "Col03").replace("0000#", "0002#");
        assert!(same_receiver(selected, &candidate));
        assert!(!same_receiver(
            selected,
            &candidate.replace("2aa4ce89", "foreign")
        ));
        assert!(same_receiver(
            selected,
            &candidate.replace("Col03", "Col04")
        ));
        assert!(!same_receiver("invalid", "invalid"));
    }
}

#[cfg(test)]
mod collection_tests {
    use super::*;
    #[test]
    fn receiver_status_skips_sibling_collections_with_other_usage() {
        let query = ReceiverStatusQuery {
            usage_page: 0x99,
            usage: 1,
            report_id: 7,
            request: vec![7, 0],
        };
        let mut caps = HidpCaps::default();
        caps.usage_page = 0x88;
        caps.usage = 1;
        assert!(!declared_collection_usage(&query, &caps));
        caps.usage_page = 0x99;
        assert!(declared_collection_usage(&query, &caps));
        caps.usage = 2;
        assert!(!declared_collection_usage(&query, &caps));
    }
}
