use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::Instant;

use super::process::{cancellation_target, LeaderOwnership};

#[derive(Debug)]
struct Member {
    pid: u32,
    exited: bool,
}

fn confirmed(
    ownership: LeaderOwnership,
    child_id: u32,
    kill_acknowledged: bool,
    members: &[Member],
) -> bool {
    cancellation_target(ownership, child_id).is_ok()
        && kill_acknowledged
        && members.iter().any(|member| member.pid == child_id)
        && members.iter().all(|member| member.exited)
}

pub(super) fn retire(
    ownership: LeaderOwnership,
    child_id: u32,
    deadline: Instant,
) -> Result<(), String> {
    let pgid = cancellation_target(ownership, child_id)?;
    loop {
        if Instant::now() >= deadline {
            return Err("probe group retirement deadline".into());
        }
        // The unreaped leader pins this group id until every member's exit is observed.
        if unsafe { libc::kill(-pgid, libc::SIGKILL) } != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                log::debug!("[blueprint] owned probe group {pgid} is absent");
                return Ok(());
            }
            return Err(error.to_string());
        }
        let snapshot = members(pgid)?;
        if !snapshot.iter().any(|member| member.pid == child_id) {
            return Err("probe pinned group leader missing from snapshot".into());
        }
        if confirmed(ownership, child_id, true, &snapshot) {
            log::debug!(
                "[blueprint] owned probe group {pgid} retired: {} exited members",
                snapshot.len()
            );
            return Ok(());
        }
        let mut events = Vec::new();
        for member in snapshot.iter().filter(|member| !member.exited) {
            match super::cancellation::unix::parent_event(member.pid) {
                Ok(fd) => events.push(unsafe { OwnedFd::from_raw_fd(fd) }),
                Err(error) => {
                    let observed = members(pgid)?;
                    if observed
                        .iter()
                        .any(|current| current.pid == member.pid && !current.exited)
                    {
                        return Err(error);
                    }
                }
            }
        }
        if events.is_empty() {
            let observed = members(pgid)?;
            return if confirmed(ownership, child_id, true, &observed) {
                Ok(())
            } else {
                Err("probe group has live members without exit events".into())
            };
        }
        wait_events(&events, deadline)?;
    }
}

fn wait_events(events: &[OwnedFd], deadline: Instant) -> Result<(), String> {
    let mut pending: Vec<_> = events
        .iter()
        .map(|event| libc::pollfd {
            fd: event.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        })
        .collect();
    while pending.iter().any(|event| event.fd >= 0) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("probe group exit event deadline".into());
        }
        let timeout = remaining
            .as_millis()
            .saturating_add(1)
            .min(i32::MAX as u128) as i32;
        let result = unsafe { libc::poll(pending.as_mut_ptr(), pending.len() as _, timeout) };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.to_string());
        }
        if result == 0 {
            return Err("probe group exit event deadline".into());
        }
        for event in &mut pending {
            if event.revents & (libc::POLLNVAL | libc::POLLERR) != 0 {
                return Err("probe group exit event invalid".into());
            }
            if event.revents != 0 {
                event.fd = -1;
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn members(pgid: i32) -> Result<Vec<Member>, String> {
    let mut members = Vec::new();
    for entry in std::fs::read_dir("/proc").map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let stat = match std::fs::read_to_string(entry.path().join("stat")) {
            Ok(stat) => stat,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.to_string()),
        };
        if let Some(member) = member_from_stat(pid, &stat, pgid)? {
            members.push(member);
        }
    }
    Ok(members)
}

#[cfg(any(target_os = "linux", test))]
fn member_from_stat(pid: u32, stat: &str, pgid: i32) -> Result<Option<Member>, String> {
    let fields = stat
        .rsplit_once(')')
        .ok_or("probe invalid process stat")?
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    let group = fields
        .get(2)
        .ok_or("probe missing process group")?
        .parse::<i32>()
        .map_err(|error| error.to_string())?;
    Ok((group == pgid).then(|| Member {
        pid,
        exited: matches!(fields.first().copied(), Some("Z" | "X")),
    }))
}

#[cfg(target_os = "macos")]
fn members(pgid: i32) -> Result<Vec<Member>, String> {
    const PROC_PGRP_ONLY: u32 = 2;
    let count =
        unsafe { libc::proc_listpids(PROC_PGRP_ONLY, pgid as u32, std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    let mut pids = vec![0u32; count as usize / std::mem::size_of::<u32>() + 64];
    let bytes = std::mem::size_of_val(pids.as_slice()) as i32;
    let filled = unsafe {
        libc::proc_listpids(PROC_PGRP_ONLY, pgid as u32, pids.as_mut_ptr().cast(), bytes)
    };
    if filled <= 0 || filled >= bytes || filled as usize % std::mem::size_of::<u32>() != 0 {
        return Err("probe incomplete process group snapshot".into());
    }
    pids.truncate(filled as usize / std::mem::size_of::<u32>());
    let mut members = Vec::new();
    for pid in pids.into_iter().filter(|pid| *pid != 0) {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of_val(&info) as i32;
        let written = unsafe {
            libc::proc_pidinfo(
                pid as i32,
                libc::PROC_PIDTBSDINFO,
                1,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        };
        if written != size {
            if io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
                continue;
            }
            return Err(io::Error::last_os_error().to_string());
        }
        if info.pbi_pgid == pgid as u32 {
            members.push(Member {
                pid,
                exited: info.pbi_status == libc::SZOMB,
            });
        }
    }
    Ok(members)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn members(_: i32) -> Result<Vec<Member>, String> {
    Err("probe process group retirement unsupported platform".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_distinguishes_live_zombie_and_foreign_group() {
        for (state, exited) in [("S", false), ("Z", true), ("X", true)] {
            let stat = format!("43 (child (with spaces)) {state} 1 42 42");
            let member = member_from_stat(43, &stat, 42).unwrap().unwrap();
            assert_eq!(member.pid, 43);
            assert_eq!(member.exited, exited);
            assert!(member_from_stat(43, &stat, 99).unwrap().is_none());
        }
        assert!(member_from_stat(43, "incomplete", 42).is_err());
    }

    #[test]
    fn retirement_requires_owned_kill_and_every_member_exit() {
        let mut members = vec![
            Member {
                pid: 42,
                exited: true,
            },
            Member {
                pid: 43,
                exited: false,
            },
        ];
        assert!(!confirmed(
            LeaderOwnership::Retained(42),
            42,
            true,
            &members
        ));
        members[1].exited = true;
        assert!(!confirmed(LeaderOwnership::Lost, 42, true, &members));
        assert!(!confirmed(
            LeaderOwnership::Retained(42),
            42,
            false,
            &members
        ));
        assert!(confirmed(LeaderOwnership::Retained(42), 42, true, &members));
        assert!(!confirmed(LeaderOwnership::Retained(42), 42, true, &[]));
        for ownership in [LeaderOwnership::Retired(42), LeaderOwnership::Lost] {
            assert!(retire(ownership, 42, Instant::now()).is_err());
        }
    }
}
