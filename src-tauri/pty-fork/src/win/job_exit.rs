use filedescriptor::OwnedHandle;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::ptr;
use winapi::shared::winerror::{ERROR_INVALID_PARAMETER, ERROR_MORE_DATA};
use winapi::um::jobapi::IsProcessInJob;
use winapi::um::jobapi2::QueryInformationJobObject;
use winapi::um::processthreadsapi::OpenProcess;
use winapi::um::synchapi::WaitForSingleObject;
use winapi::um::winbase::{INFINITE, WAIT_OBJECT_0};
use winapi::um::winnt::{
    JobObjectBasicAccountingInformation, JobObjectBasicProcessIdList,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_BASIC_PROCESS_ID_LIST,
    PROCESS_QUERY_LIMITED_INFORMATION, SYNCHRONIZE,
};

pub(super) fn is_empty(job: &OwnedHandle) -> io::Result<bool> {
    let mut accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe {
        QueryInformationJobObject(
            job.as_raw_handle() as _,
            JobObjectBasicAccountingInformation,
            &mut accounting as *mut _ as _,
            std::mem::size_of_val(&accounting) as u32,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(accounting.ActiveProcesses == 0)
}

pub(super) fn process_ids(job: &OwnedHandle) -> io::Result<Vec<u32>> {
    let mut storage = vec![0usize; 32];
    loop {
        let result = unsafe {
            QueryInformationJobObject(
                job.as_raw_handle() as _,
                JobObjectBasicProcessIdList,
                storage.as_mut_ptr() as _,
                (storage.len() * std::mem::size_of::<usize>()) as u32,
                ptr::null_mut(),
            )
        };
        let list = unsafe { &*(storage.as_ptr() as *const JOBOBJECT_BASIC_PROCESS_ID_LIST) };
        if result != 0 {
            let ids = unsafe {
                std::slice::from_raw_parts(
                    list.ProcessIdList.as_ptr(),
                    list.NumberOfProcessIdsInList as usize,
                )
            };
            return Ok(ids.iter().map(|id| *id as u32).collect());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_MORE_DATA as i32) {
            return Err(error);
        }
        storage.resize(list.NumberOfAssignedProcesses as usize + 2, 0);
    }
}

pub(super) fn wait(job: &OwnedHandle) -> io::Result<()> {
    loop {
        if is_empty(job)? {
            return Ok(());
        }
        let ids = process_ids(job)?;
        for pid in ids {
            let raw =
                unsafe { OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            if raw.is_null() {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
                    continue;
                }
                return Err(error);
            }
            let process = unsafe { OwnedHandle::from_raw_handle(raw as _) };
            let mut belongs = 0;
            if unsafe { IsProcessInJob(raw, job.as_raw_handle() as _, &mut belongs) } == 0 {
                return Err(io::Error::last_os_error());
            }
            // Pin and verify membership before waiting: an enumerated PID can be reused.
            if belongs != 0 && unsafe { WaitForSingleObject(raw, INFINITE) } != WAIT_OBJECT_0 {
                return Err(io::Error::last_os_error());
            }
            drop(process);
        }
    }
}
