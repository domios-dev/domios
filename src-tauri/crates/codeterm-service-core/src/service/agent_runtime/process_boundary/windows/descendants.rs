use super::{Boundary, Member};
use std::collections::{HashMap, HashSet};
use windows_sys::Win32::{
    Foundation::*,
    System::{Diagnostics::ToolHelp::*, Threading::*},
};

pub(in crate::service::agent_runtime::process_boundary) fn capture(
    boundary: &Boundary,
) -> Result<Vec<Member>, String> {
    let root = boundary
        .infrastructure
        .get(&boundary.pid)
        .ok_or("Runtime owner witness unavailable")?;
    let parents = parents()?;
    let mut pending = vec![boundary.pid];
    let mut retained = HashMap::new();
    let mut visited = HashSet::from([boundary.pid]);
    while !pending.is_empty() {
        let mut next = Vec::new();
        for parent_pid in pending {
            let parent = if parent_pid == boundary.pid {
                root
            } else {
                &retained[&parent_pid]
            };
            for (&pid, _) in parents.iter().filter(|(_, owner)| **owner == parent_pid) {
                if !visited.insert(pid) {
                    return Err("Cyclic runtime descendant ancestry".into());
                }
                let Some(child) = Member::retain(pid)? else {
                    continue;
                };
                // A recycled parent PID names an unrelated older process.
                if !parent.owns_child(&child)? {
                    continue;
                }
                if retained.len() + next.len() >= 256 {
                    return Err("Runtime descendant snapshot exceeds 256 members".into());
                }
                next.push((pid, child));
            }
        }
        pending = next.iter().map(|(pid, _)| *pid).collect();
        retained.extend(next);
    }
    Ok(retained.into_values().collect())
}

fn parents() -> Result<HashMap<u32, u32>, String> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let result = (|| {
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of_val(&entry) as u32;
        let mut entries = HashMap::new();
        let mut available = unsafe { Process32FirstW(snapshot, &mut entry) };
        while available != 0 {
            entries.insert(entry.th32ProcessID, entry.th32ParentProcessID);
            available = unsafe { Process32NextW(snapshot, &mut entry) };
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
            return Err(format!("Process32NextW: {error}"));
        }
        Ok(entries)
    })();
    unsafe { CloseHandle(snapshot) };
    result
}

impl Member {
    fn owns_child(&self, child: &Self) -> Result<bool, String> {
        let mut born = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        if unsafe {
            GetProcessTimes(
                self.handle as HANDLE,
                &mut born,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let exited = (u128::from(exited.dwHighDateTime) << 32) | u128::from(exited.dwLowDateTime);
        Ok(child.born >= self.born && (exited == 0 || child.born <= exited))
    }
}
