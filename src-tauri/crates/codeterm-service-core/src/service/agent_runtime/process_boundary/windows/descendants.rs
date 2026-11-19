use super::{Boundary, Member};
use std::collections::{HashMap, HashSet};
use windows_sys::Win32::{
    Foundation::*,
    System::{Diagnostics::ToolHelp::*, Threading::*},
};

pub(in crate::service::agent_runtime::process_boundary) struct Descendant {
    pub(in crate::service::agent_runtime::process_boundary) pid: u32,
    pub(in crate::service::agent_runtime::process_boundary) parent: u32,
    pub(in crate::service::agent_runtime::process_boundary) member: Member,
}

pub(in crate::service::agent_runtime::process_boundary) fn capture(
    boundary: &Boundary,
) -> Result<Vec<Member>, String> {
    Ok(tree(boundary)?
        .into_iter()
        .map(|descendant| descendant.member)
        .collect())
}

/// Every live runtime descendant with its parent, in or outside the owned Job.
pub(in crate::service::agent_runtime::process_boundary) fn tree(
    boundary: &Boundary,
) -> Result<Vec<Descendant>, String> {
    let root = boundary
        .infrastructure
        .get(&boundary.pid)
        .ok_or("Runtime owner witness unavailable")?;
    below(boundary.pid, root, &parents()?)
}

/// The live descendants of one retained process, taken from a fresh snapshot.
pub(in crate::service::agent_runtime::process_boundary) fn of(
    pid: u32,
    member: &Member,
) -> Result<Vec<Descendant>, String> {
    below(pid, member, &parents()?)
}

fn below(
    root_pid: u32,
    root: &Member,
    parents: &HashMap<u32, u32>,
) -> Result<Vec<Descendant>, String> {
    let mut pending = vec![root_pid];
    let mut retained: HashMap<u32, Descendant> = HashMap::new();
    let mut visited = HashSet::from([root_pid]);
    while !pending.is_empty() {
        let mut next = Vec::new();
        for parent_pid in pending {
            let parent = if parent_pid == root_pid {
                root
            } else {
                &retained[&parent_pid].member
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
                next.push(Descendant {
                    pid,
                    parent: parent_pid,
                    member: child,
                });
            }
        }
        pending = next.iter().map(|descendant| descendant.pid).collect();
        retained.extend(
            next.into_iter()
                .map(|descendant| (descendant.pid, descendant)),
        );
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
