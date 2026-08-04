//! Raise the open-file soft limit at startup. A busy session (many tabs ×
//! several PTY fds each + plugins + sqlite + mesh sockets) blows past the
//! shell's default 256 soft limit and hits EMFILE ("Too many open files"),
//! which silently breaks plugin loads and socket accepts (#186). GUI launches
//! inherit a high limit; terminal-launched dev does not.

#[cfg(unix)]
pub fn raise_fd_limit() {
    // kern.maxfilesperproc so setrlimit won't EINVAL.
    const TARGET: libc::rlim_t = 65536;
    unsafe {
        let mut lim = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim) != 0 {
            log::warn!(
                "[fdlimit] getrlimit failed: {}",
                std::io::Error::last_os_error()
            );
            return;
        }
        let desired = if lim.rlim_max == libc::RLIM_INFINITY {
            TARGET
        } else {
            TARGET.min(lim.rlim_max)
        };
        if lim.rlim_cur >= desired {
            return;
        }
        let old = lim.rlim_cur;
        lim.rlim_cur = desired;
        if libc::setrlimit(libc::RLIMIT_NOFILE, &lim) != 0 {
            log::warn!(
                "[fdlimit] setrlimit to {desired} failed: {}",
                std::io::Error::last_os_error()
            );
            return;
        }
        log::info!("[fdlimit] open-file soft limit raised {old} -> {desired}");
    }
}

#[cfg(not(unix))]
pub fn raise_fd_limit() {}

#[cfg(all(test, unix))]
mod tests {
    fn soft_limit() -> libc::rlim_t {
        let mut lim = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        unsafe {
            assert_eq!(libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim), 0);
        }
        lim.rlim_cur
    }

    #[test]
    fn raise_never_lowers_and_lifts_above_default() {
        let before = soft_limit();
        super::raise_fd_limit();
        let after = soft_limit();
        assert!(after >= before, "soft limit dropped: {before} -> {after}");
        assert!(
            after >= 1024,
            "soft limit not lifted clear of EMFILE: {after}"
        );
    }
}
