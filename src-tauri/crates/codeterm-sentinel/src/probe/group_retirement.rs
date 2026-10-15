use std::collections::BTreeSet;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::Instant;

use super::process::{cancellation_target, LeaderOwnership};

#[derive(Debug)]
struct Member {
    pid: u32,
    born: u128,
    exited: bool,
}

fn confirmed(ownership: LeaderOwnership, child_id: u32, members: &[Member]) -> bool {
    cancellation_target(ownership, child_id).is_ok() && members.iter().all(|member| member.exited)
}

fn signal_result(result: i32, errno: i32) -> io::Result<bool> {
    match (result, errno) {
        (0, _) => Ok(false),
        (-1, libc::ESRCH) => Ok(true),
        _ => Err(io::Error::from_raw_os_error(errno)),
    }
}

fn reconcile_signal(result: io::Result<bool>, snapshot: &[Member]) -> io::Result<bool> {
    match result {
        Err(error)
            if error.raw_os_error() == Some(libc::EPERM)
                && snapshot.iter().all(|member| member.exited) =>
        {
            Ok(true)
        }
        result => result,
    }
}

pub fn signal_owned_group(ownership: LeaderOwnership, child_id: u32) -> Result<bool, String> {
    let pgid = cancellation_target(ownership, child_id)?;
    checked_group_signal(pgid, libc::SIGKILL)
}

fn group_signal(pgid: i32, signal: i32) -> io::Result<bool> {
    let result = unsafe { libc::kill(-pgid, signal) };
    signal_result(
        result,
        io::Error::last_os_error().raw_os_error().unwrap_or(0),
    )
}

fn checked_group_signal(pgid: i32, signal: i32) -> Result<bool, String> {
    let result = group_signal(pgid, signal);
    let result = if result
        .as_ref()
        .is_err_and(|error| error.raw_os_error() == Some(libc::EPERM))
    {
        let snapshot = members(pgid)?;
        reconcile_signal(result, &snapshot)
    } else {
        result
    };
    result.map_err(|error| format!("probe group {pgid} signal {signal}: {error}"))
}

pub fn observe_retired(ownership: LeaderOwnership, child_id: u32) -> Result<bool, String> {
    if ownership != LeaderOwnership::Retired(child_id)
        || child_id <= 1
        || child_id > i32::MAX as u32
    {
        return Err("probe group observation requires an observed leader exit".into());
    }
    let pgid = child_id as i32;
    if checked_group_signal(pgid, 0)? {
        return Ok(true);
    }
    members(pgid).map(|snapshot| snapshot.iter().all(|member| member.exited))
}

fn apply_exit_events(members: &mut [Member], exited: &BTreeSet<(u32, u128)>) {
    for member in members {
        member.exited |= exited.contains(&(member.pid, member.born));
    }
}

fn registration_race_retired(member: &Member, snapshot: &[Member]) -> bool {
    !snapshot
        .iter()
        .any(|current| current.pid == member.pid && current.born == member.born && !current.exited)
}

pub fn retire(ownership: LeaderOwnership, child_id: u32, deadline: Instant) -> Result<(), String> {
    let pgid = cancellation_target(ownership, child_id)?;
    let mut exited = BTreeSet::new();
    loop {
        if Instant::now() >= deadline {
            return Err("probe group retirement deadline".into());
        }
        let mut snapshot = members(pgid)?;
        apply_exit_events(&mut snapshot, &exited);
        if confirmed(ownership, child_id, &snapshot) {
            log::debug!(
                "[blueprint] owned probe group {pgid} retired: {} exited members",
                snapshot.len()
            );
            return Ok(());
        }
        let mut events = Vec::new();
        for member in snapshot.iter().filter(|member| !member.exited) {
            match super::cancellation::unix::parent_event(member.pid) {
                Ok(fd) => events.push(((member.pid, member.born), unsafe {
                    OwnedFd::from_raw_fd(fd)
                })),
                Err(error) => {
                    if error.raw_os_error() != Some(libc::ESRCH) {
                        return Err(format!(
                            "probe member {} exit registration: {error}",
                            member.pid
                        ));
                    }
                    let mut observed = members(pgid)?;
                    apply_exit_events(&mut observed, &exited);
                    if !registration_race_retired(member, &observed) {
                        let result = unsafe { libc::kill(member.pid as i32, 0) };
                        if !signal_result(
                            result,
                            io::Error::last_os_error().raw_os_error().unwrap_or(0),
                        )
                        .map_err(|error| error.to_string())?
                        {
                            return Err(format!(
                                "probe member {} exit registration unconfirmed: {error}",
                                member.pid
                            ));
                        }
                    }
                    exited.insert((member.pid, member.born));
                }
            }
        }
        if events.is_empty() {
            let mut observed = members(pgid)?;
            apply_exit_events(&mut observed, &exited);
            return if confirmed(ownership, child_id, &observed) {
                Ok(())
            } else {
                Err("probe group has live members without exit events".into())
            };
        }
        // Arm every member's exit event before sending the signal that may make registration impossible.
        if signal_owned_group(ownership, child_id)? {
            return Ok(());
        }
        wait_events(&events, deadline)?;
        log::debug!(
            "[blueprint] owned probe group {pgid} observed {} member exits",
            events.len()
        );
        exited.extend(events.iter().map(|(identity, _)| *identity));
    }
}

fn wait_events(events: &[((u32, u128), OwnedFd)], deadline: Instant) -> Result<(), String> {
    let mut pending: Vec<_> = events
        .iter()
        .map(|(_, event)| libc::pollfd {
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
        for (index, event) in pending.iter_mut().enumerate() {
            if event.revents & (libc::POLLNVAL | libc::POLLERR) != 0 {
                return Err("probe group exit event invalid".into());
            }
            if event.revents & libc::POLLIN != 0 {
                let ((pid, _), _) = &events[index];
                verify_exit_event(event.fd, *pid)?;
                event.fd = -1;
            } else if event.revents != 0 {
                return Err("probe group exit event unexpected".into());
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn verify_exit_event(fd: i32, pid: u32) -> Result<(), String> {
    let mut event: libc::kevent = unsafe { std::mem::zeroed() };
    let timeout = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let result = unsafe { libc::kevent(fd, std::ptr::null(), 0, &mut event, 1, &timeout) };
    if result == 1
        && event.ident == pid as libc::uintptr_t
        && event.filter == libc::EVFILT_PROC
        && event.fflags & libc::NOTE_EXIT != 0
        && event.flags & libc::EV_ERROR == 0
    {
        Ok(())
    } else {
        Err(format!("probe member {pid} exit event unconfirmed"))
    }
}

#[cfg(not(target_os = "macos"))]
fn verify_exit_event(_: i32, _: u32) -> Result<(), String> {
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
        let Some(stat) = live_stat(std::fs::read_to_string(entry.path().join("stat")))? else {
            continue;
        };
        if let Some(member) = member_from_stat(pid, &stat, pgid)? {
            members.push(member);
        }
    }
    Ok(members)
}

// procfs returns ESRCH, not ENOENT, when the task is reaped between readdir and read.
#[cfg(any(target_os = "linux", test))]
fn live_stat(read: io::Result<String>) -> Result<Option<String>, String> {
    match read {
        Ok(stat) => Ok(Some(stat)),
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                || error.raw_os_error() == Some(libc::ESRCH) =>
        {
            Ok(None)
        }
        Err(error) => Err(error.to_string()),
    }
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
    if group != pgid {
        return Ok(None);
    }
    let born = fields
        .get(19)
        .ok_or("probe missing process birth time")?
        .parse::<u128>()
        .map_err(|error| error.to_string())?;
    Ok(Some(Member {
        pid,
        born,
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
    unsafe {
        *libc::__error() = 0;
    }
    let filled = unsafe {
        libc::proc_listpids(PROC_PGRP_ONLY, pgid as u32, pids.as_mut_ptr().cast(), bytes)
    };
    let errno = io::Error::last_os_error().raw_os_error().unwrap_or(0);
    pids.truncate(snapshot_count(filled, bytes, errno)?);
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
        let born = u128::from(info.pbi_start_tvsec) * 1_000_000 + u128::from(info.pbi_start_tvusec);
        if let Some(member) = member_from_bsd_snapshot(
            pid,
            born,
            info.pbi_pgid,
            pgid as u32,
            info.pbi_status == libc::SZOMB,
        ) {
            members.push(member);
        }
    }
    Ok(members)
}

#[cfg(any(target_os = "macos", test))]
fn snapshot_count(filled: i32, capacity: i32, errno: i32) -> Result<usize, String> {
    if filled < 0
        || filled >= capacity
        || filled as usize % std::mem::size_of::<u32>() != 0
        || (filled == 0 && errno != 0)
    {
        return Err(format!(
            "probe incomplete process group snapshot: bytes={filled}, errno={errno}"
        ));
    }
    Ok(filled as usize / std::mem::size_of::<u32>())
}

#[cfg(any(target_os = "macos", test))]
fn member_from_bsd_snapshot(
    pid: u32,
    born: u128,
    group: u32,
    pgid: u32,
    exited: bool,
) -> Option<Member> {
    // XNU's full BSD info can omit the group id after the zombie loses its session.
    (group == pgid || (group == 0 && exited)).then_some(Member { pid, born, exited })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn members(_: i32) -> Result<Vec<Member>, String> {
    Err("probe process group retirement unsupported platform".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_signal_is_retirement_evidence_but_other_errors_are_not() {
        assert!(signal_result(-1, libc::ESRCH).unwrap());
        assert!(!signal_result(0, libc::ESRCH).unwrap());
        for errno in [libc::EPERM, libc::EINVAL] {
            assert_eq!(
                signal_result(-1, errno).unwrap_err().raw_os_error(),
                Some(errno)
            );
        }
    }

    #[test]
    fn reaped_process_stat_is_absence_but_other_read_errors_fail() {
        assert_eq!(live_stat(Ok("s".into())).unwrap().as_deref(), Some("s"));
        for errno in [libc::ENOENT, libc::ESRCH] {
            assert_eq!(
                live_stat(Err(io::Error::from_raw_os_error(errno))),
                Ok(None)
            );
        }
        assert!(live_stat(Err(io::Error::from_raw_os_error(libc::EACCES))).is_err());
    }

    #[test]
    fn permission_denial_requires_a_complete_exited_snapshot() {
        let mut snapshot = vec![Member {
            pid: 42,
            born: 1,
            exited: false,
        }];
        let denied = || io::Error::from_raw_os_error(libc::EPERM);
        assert_eq!(
            reconcile_signal(Err(denied()), &snapshot)
                .unwrap_err()
                .raw_os_error(),
            Some(libc::EPERM)
        );
        snapshot[0].exited = true;
        assert!(reconcile_signal(Err(denied()), &snapshot).unwrap());
        assert!(reconcile_signal(Err(denied()), &[]).unwrap());
        assert_eq!(
            reconcile_signal(Err(io::Error::from_raw_os_error(libc::EINVAL)), &snapshot)
                .unwrap_err()
                .raw_os_error(),
            Some(libc::EINVAL)
        );
        assert!(!reconcile_signal(Ok(false), &snapshot).unwrap());
    }

    #[test]
    fn missing_exit_registration_requires_independent_member_retirement_proof() {
        let member = Member {
            pid: 42,
            born: 1,
            exited: false,
        };
        let mut snapshot = vec![Member {
            pid: 42,
            born: 1,
            exited: false,
        }];
        assert!(!registration_race_retired(&member, &snapshot));
        snapshot[0].exited = true;
        assert!(registration_race_retired(&member, &snapshot));
        snapshot[0].exited = false;
        snapshot[0].born = 2;
        assert!(registration_race_retired(&member, &snapshot));
        assert!(registration_race_retired(&member, &[]));
    }

    #[test]
    fn empty_group_snapshot_requires_a_successful_complete_call() {
        assert_eq!(snapshot_count(0, 256, 0).unwrap(), 0);
        assert_eq!(snapshot_count(8, 256, 0).unwrap(), 2);
        for (filled, capacity, errno) in [
            (0, 256, libc::EPERM),
            (-1, 256, 0),
            (256, 256, 0),
            (3, 256, 0),
        ] {
            assert!(snapshot_count(filled, capacity, errno).is_err());
        }
    }

    #[test]
    fn observed_exit_survives_snapshot_before_zombie_state() {
        let mut members = vec![
            Member {
                pid: 42,
                born: 1,
                exited: false,
            },
            Member {
                pid: 43,
                born: 2,
                exited: false,
            },
        ];
        let mut exited = BTreeSet::from([(42, 1)]);
        apply_exit_events(&mut members, &exited);
        assert!(!confirmed(LeaderOwnership::Retained(42), 42, &members));
        exited.insert((43, 2));
        apply_exit_events(&mut members, &exited);
        assert!(confirmed(LeaderOwnership::Retained(42), 42, &members));
        members[1].born = 3;
        members[1].exited = false;
        apply_exit_events(&mut members, &exited);
        assert!(!confirmed(LeaderOwnership::Retained(42), 42, &members));
    }

    #[test]
    fn bsd_snapshot_retains_sessionless_zombies_without_accepting_live_foreign_members() {
        let zombie = member_from_bsd_snapshot(42, 123, 0, 42, true).unwrap();
        assert!(zombie.exited);
        assert_eq!(zombie.born, 123);
        assert!(member_from_bsd_snapshot(42, 123, 0, 42, false).is_none());
        assert!(member_from_bsd_snapshot(43, 123, 99, 42, true).is_none());
        assert!(
            !member_from_bsd_snapshot(43, 123, 42, 42, false)
                .unwrap()
                .exited
        );
    }

    #[test]
    fn snapshot_distinguishes_live_zombie_and_foreign_group() {
        for (state, exited) in [("S", false), ("Z", true), ("X", true)] {
            let stat = format!(
                "43 (child (with spaces)) {state} 1 42 42 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 123"
            );
            let member = member_from_stat(43, &stat, 42).unwrap().unwrap();
            assert_eq!(member.pid, 43);
            assert_eq!(member.born, 123);
            assert_eq!(member.exited, exited);
            assert!(member_from_stat(43, &stat, 99).unwrap().is_none());
        }
        assert!(member_from_stat(43, "incomplete", 42).is_err());
    }

    #[test]
    fn retirement_requires_retained_ownership_and_every_member_exit() {
        let mut members = vec![
            Member {
                pid: 42,
                born: 1,
                exited: true,
            },
            Member {
                pid: 43,
                born: 2,
                exited: false,
            },
        ];
        assert!(!confirmed(LeaderOwnership::Retained(42), 42, &members));
        members[1].exited = true;
        assert!(!confirmed(LeaderOwnership::Lost, 42, &members));
        assert!(confirmed(LeaderOwnership::Retained(42), 42, &members));
        assert!(confirmed(LeaderOwnership::Retained(42), 42, &[]));
        assert!(!confirmed(LeaderOwnership::Retired(42), 42, &[]));
        for ownership in [LeaderOwnership::Retired(42), LeaderOwnership::Lost] {
            assert!(retire(ownership, 42, Instant::now()).is_err());
        }
    }
}
