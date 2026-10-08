use serde_json::{json, Value};
use std::{io, os::windows::io::AsRawHandle};
use winapi::um::{
    handleapi::{CloseHandle, INVALID_HANDLE_VALUE},
    processthreadsapi::{GetProcessId, GetProcessTimes, OpenThread},
    tlhelp32::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    },
    winnt::{HANDLE, THREAD_QUERY_INFORMATION},
};

#[link(name = "ntdll")]
extern "system" {
    fn NtQueryInformationThread(
        thread: HANDLE,
        class: u32,
        data: *mut u32,
        size: u32,
        returned: *mut u32,
    ) -> i32;
}

pub fn snapshot(child: &impl AsRawHandle) -> Value {
    let pid = unsafe { GetProcessId(child.as_raw_handle().cast()) };
    let cpu = unsafe {
        let mut created = std::mem::zeroed();
        let mut exited = std::mem::zeroed();
        let mut kernel = std::mem::zeroed();
        let mut user = std::mem::zeroed();
        if GetProcessTimes(
            child.as_raw_handle().cast(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        ) == 0
        {
            json!({"error": io::Error::last_os_error().to_string()})
        } else {
            json!({
                "kernel_100ns": (u64::from(kernel.dwHighDateTime) << 32) | u64::from(kernel.dwLowDateTime),
                "user_100ns": (u64::from(user.dwHighDateTime) << 32) | u64::from(user.dwLowDateTime)
            })
        }
    };
    json!({"pid": pid, "cpu": cpu, "threads": threads(pid)})
}

fn threads(pid: u32) -> Value {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return json!({"error": io::Error::last_os_error().to_string()});
        }
        let mut entry: THREADENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of_val(&entry) as u32;
        let mut found = Thread32First(snapshot, &mut entry);
        let mut result = Vec::new();
        while found != 0 {
            if entry.th32OwnerProcessID == pid {
                let thread = OpenThread(THREAD_QUERY_INFORMATION, 0, entry.th32ThreadID);
                let state = if thread.is_null() {
                    json!({"id": entry.th32ThreadID, "error": io::Error::last_os_error().to_string()})
                } else {
                    let mut count = 0;
                    // ThreadSuspendCount is a read-only query; never perturb the child's suspend count.
                    let status =
                        NtQueryInformationThread(thread, 35, &mut count, 4, std::ptr::null_mut());
                    CloseHandle(thread);
                    json!({"id": entry.th32ThreadID, "query_status": status, "suspend_count": if status >= 0 { Some(count) } else { None }})
                };
                result.push(state);
            }
            found = Thread32Next(snapshot, &mut entry);
        }
        let error = io::Error::last_os_error();
        CloseHandle(snapshot);
        json!({"entries": result, "enumeration_end": error.raw_os_error()})
    }
}
