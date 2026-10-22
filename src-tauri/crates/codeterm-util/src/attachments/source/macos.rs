use std::ffi::CString;
use std::fs::Metadata;
use std::mem::size_of;
use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
use std::path::Path;
use std::time::{Duration, Instant};

#[repr(C)]
struct FileInfo {
    flags: u32,
    _status: u32,
    _offset: libc::off_t,
    _kind: i32,
    _guard_flags: u32,
}

#[repr(C)]
struct VnodeFdInfo {
    file: FileInfo,
    vnode: libc::vnode_info,
}

#[link(name = "proc")]
unsafe extern "C" {
    fn proc_listpidspath(
        kind: u32,
        type_info: u32,
        path: *const libc::c_char,
        flags: u32,
        buffer: *mut libc::c_void,
        bytes: libc::c_int,
    ) -> libc::c_int;
}

pub(super) fn ensure_no_writer(path: &Path, metadata: &Metadata) -> Result<(), String> {
    let path = CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let mut pids = vec![0i32; 4096];
    let capacity = size_of::<i32>() * pids.len();
    let bytes = unsafe {
        proc_listpidspath(
            1,
            0,
            path.as_ptr(),
            2,
            pids.as_mut_ptr().cast(),
            capacity as i32,
        )
    };
    if bytes < 0 || bytes as usize >= capacity || !(bytes as usize).is_multiple_of(size_of::<i32>())
    {
        return Err("Cannot verify attachment writer handles; close the file and try again".into());
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    for pid in pids
        .into_iter()
        .take(bytes as usize / size_of::<i32>())
        .filter(|pid| *pid > 0)
    {
        if Instant::now() >= deadline {
            return Err("Attachment writer inspection exceeded its deadline".into());
        }
        ensure_process_has_no_writer(pid, metadata, deadline)?;
    }
    Ok(())
}

fn ensure_process_has_no_writer(
    pid: i32,
    metadata: &Metadata,
    deadline: Instant,
) -> Result<(), String> {
    let mut descriptors: Vec<libc::proc_fdinfo> =
        (0..8192).map(|_| unsafe { std::mem::zeroed() }).collect();
    let capacity = size_of::<libc::proc_fdinfo>() * descriptors.len();
    let bytes = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDLISTFDS,
            0,
            descriptors.as_mut_ptr().cast(),
            capacity as i32,
        )
    };
    if bytes <= 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        return Err(format!("Cannot verify attachment writer handles: {error}"));
    }
    if bytes as usize >= capacity
        || !(bytes as usize).is_multiple_of(size_of::<libc::proc_fdinfo>())
    {
        return Err("Cannot verify attachment writer handles: too many open files".into());
    }
    for descriptor in descriptors
        .into_iter()
        .take(bytes as usize / size_of::<libc::proc_fdinfo>())
    {
        if Instant::now() >= deadline {
            return Err("Attachment writer inspection exceeded its deadline".into());
        }
        if descriptor.proc_fdtype != libc::PROX_FDTYPE_VNODE as u32 {
            continue;
        }
        let mut info = std::mem::MaybeUninit::<VnodeFdInfo>::zeroed();
        let bytes = unsafe {
            libc::proc_pidfdinfo(
                pid,
                descriptor.proc_fd,
                1,
                info.as_mut_ptr().cast(),
                size_of::<VnodeFdInfo>() as i32,
            )
        };
        if bytes != size_of::<VnodeFdInfo>() as i32 {
            let error = std::io::Error::last_os_error();
            if matches!(
                error.raw_os_error(),
                Some(libc::EBADF | libc::ESRCH | libc::ENOENT)
            ) {
                continue;
            }
            return Err(format!("Cannot verify attachment writer handles: {error}"));
        }
        let info = unsafe { info.assume_init() };
        if info.file.flags & 2 != 0
            && info.vnode.vi_stat.vst_dev as u64 == metadata.dev()
            && info.vnode.vi_stat.vst_ino == metadata.ino()
        {
            return Err(super::WRITING.into());
        }
    }
    Ok(())
}
