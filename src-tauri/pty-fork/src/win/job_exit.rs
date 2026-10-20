use filedescriptor::OwnedHandle;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::ptr;
use std::sync::Mutex;
use winapi::shared::winerror::{ERROR_INVALID_PARAMETER, ERROR_MORE_DATA};
use winapi::um::jobapi::IsProcessInJob;
use winapi::um::jobapi2::{QueryInformationJobObject, TerminateJobObject};
use winapi::um::processthreadsapi::{OpenProcess, TerminateProcess};
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

fn pin_members(job: &OwnedHandle) -> io::Result<Vec<OwnedHandle>> {
    let mut members = Vec::new();
    for pid in process_ids(job)? {
        let raw = unsafe { OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
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
        if belongs != 0 {
            members.push(process);
        }
    }
    Ok(members)
}

#[derive(Debug, Default)]
pub(super) struct Termination {
    members: Mutex<Option<Vec<OwnedHandle>>>,
}

impl Termination {
    pub(super) fn kill(&self, job: &OwnedHandle, leader: &OwnedHandle) -> io::Result<()> {
        let mut members = self
            .members
            .lock()
            .map_err(|_| io::Error::other("Job termination mutex poisoned"))?;
        // Job accounting can reach zero before process handles signal, so pin members before killing.
        let mut pinned = pin_members(job)?;
        if unsafe { TerminateJobObject(job.as_raw_handle() as _, 1) } == 0 {
            let error = io::Error::last_os_error();
            unsafe { TerminateProcess(leader.as_raw_handle() as _, 1) };
            return Err(error);
        }
        members.get_or_insert_with(Vec::new).append(&mut pinned);
        Ok(())
    }

    pub(super) fn is_complete(&self, job: &OwnedHandle) -> io::Result<bool> {
        let members = self
            .members
            .lock()
            .map_err(|_| io::Error::other("Job termination mutex poisoned"))?;
        let Some(members) = members.as_ref() else {
            return Ok(true);
        };
        for member in members {
            match unsafe { WaitForSingleObject(member.as_raw_handle() as _, 0) } {
                WAIT_OBJECT_0 => {}
                winapi::shared::winerror::WAIT_TIMEOUT => return Ok(false),
                _ => return Err(io::Error::last_os_error()),
            }
        }
        is_empty(job)
    }

    pub(super) fn wait(&self, job: &OwnedHandle) -> io::Result<()> {
        let pinned = {
            let members = self
                .members
                .lock()
                .map_err(|_| io::Error::other("Job termination mutex poisoned"))?;
            let Some(members) = members.as_ref() else {
                return Ok(());
            };
            members
                .iter()
                .map(|member| member.try_clone().map_err(io::Error::other))
                .collect::<io::Result<Vec<_>>>()?
        };
        for member in pinned {
            wait_member(&member)?;
        }
        while !is_empty(job)? {
            let members = pin_members(job)?;
            if members.is_empty() {
                return Err(io::Error::other(
                    "Active Job has no owned process exit handle",
                ));
            }
            for member in members {
                wait_member(&member)?;
            }
        }
        Ok(())
    }
}

fn wait_member(member: &OwnedHandle) -> io::Result<()> {
    if unsafe { WaitForSingleObject(member.as_raw_handle() as _, INFINITE) } == WAIT_OBJECT_0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
