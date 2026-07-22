use super::WinChild;
use crate::cmdbuilder::CommandBuilder;
use crate::win::procthreadattr::ProcThreadAttributeList;
use anyhow::{bail, ensure, Error};
use filedescriptor::{FileDescriptor, OwnedHandle};
use lazy_static::lazy_static;
use shared_library::shared_library;
use std::ffi::OsString;
use std::io::Error as IoError;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;
use std::{mem, ptr};
use winapi::shared::minwindef::DWORD;
use winapi::shared::winerror::{HRESULT, S_OK};
use winapi::um::handleapi::*;
use winapi::um::jobapi2::{AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject};
use winapi::um::processthreadsapi::*;
use winapi::um::winbase::{
    CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT,
    STARTF_USESHOWWINDOW, STARTF_USESTDHANDLES, STARTUPINFOEXW,
};
use winapi::um::wincon::COORD;
use winapi::um::winnt::{
    JobObjectExtendedLimitInformation, HANDLE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use winapi::um::winuser::SW_HIDE;

pub type HPCON = HANDLE;

#[allow(dead_code)]
pub const PSUEDOCONSOLE_INHERIT_CURSOR: DWORD = 0x1;
pub const PSEUDOCONSOLE_RESIZE_QUIRK: DWORD = 0x2;
#[allow(dead_code)]
pub const PSEUDOCONSOLE_WIN32_INPUT_MODE: DWORD = 0x4;
#[allow(dead_code)]
pub const PSEUDOCONSOLE_PASSTHROUGH_MODE: DWORD = 0x8;

shared_library!(ConPtyFuncs,
    pub fn CreatePseudoConsole(
        size: COORD,
        hInput: HANDLE,
        hOutput: HANDLE,
        flags: DWORD,
        hpc: *mut HPCON
    ) -> HRESULT,
    pub fn ResizePseudoConsole(hpc: HPCON, size: COORD) -> HRESULT,
    pub fn ClosePseudoConsole(hpc: HPCON),
);

// Sideload conpty.dll (+ its sibling OpenConsole.exe) by ABSOLUTE path so
// CreatePseudoConsole uses the windowless host, not the flashing in-box conhost.
fn sideloaded_conpty() -> Option<ConPtyFuncs> {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Some(d) = std::env::var_os("CODETERM_CONPTY_DIR") {
        dirs.push(std::path::PathBuf::from(d));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.to_path_buf());
        }
    }
    for dir in dirs {
        let dll = dir.join("conpty.dll");
        if dll.exists() {
            if let Ok(funcs) = ConPtyFuncs::open(&dll) {
                eprintln!("[conpty] sideloaded host: {}", dll.display());
                return Some(funcs);
            }
        }
    }
    None
}

fn load_conpty() -> ConPtyFuncs {
    // If the kernel doesn't export these functions then their system is too old and we cannot run.
    let kernel = ConPtyFuncs::open(Path::new("kernel32.dll")).expect(
        "this system does not support conpty.  Windows 10 October 2018 or newer is required",
    );
    match sideloaded_conpty() {
        Some(funcs) => funcs,
        None => {
            eprintln!("[conpty] in-box conhost (no sideloaded conpty.dll found)");
            kernel
        }
    }
}

lazy_static! {
    static ref CONPTY: ConPtyFuncs = load_conpty();
}

pub struct PsuedoCon {
    con: HPCON,
}

unsafe impl Send for PsuedoCon {}
unsafe impl Sync for PsuedoCon {}

impl Drop for PsuedoCon {
    fn drop(&mut self) {
        unsafe { (CONPTY.ClosePseudoConsole)(self.con) };
    }
}

impl PsuedoCon {
    pub fn new(size: COORD, input: FileDescriptor, output: FileDescriptor) -> Result<Self, Error> {
        let mut con: HPCON = INVALID_HANDLE_VALUE;
        let base_flags = PSEUDOCONSOLE_RESIZE_QUIRK;
        let input_handle = input.as_raw_handle();
        let output_handle = output.as_raw_handle();
        let result = unsafe {
            (CONPTY.CreatePseudoConsole)(
                size,
                input_handle as _,
                output_handle as _,
                base_flags,
                &mut con,
            )
        };
        eprintln!(
            "[conpty] CreatePseudoConsole flags=0x{:x} HRESULT=0x{:08x}",
            base_flags, result as u32,
        );
        ensure!(
            result == S_OK,
            "failed to create psuedo console: HRESULT {}",
            result
        );
        Ok(Self { con })
    }

    pub fn resize(&self, size: COORD) -> Result<(), Error> {
        let result = unsafe { (CONPTY.ResizePseudoConsole)(self.con, size) };
        ensure!(
            result == S_OK,
            "failed to resize console to {}x{}: HRESULT: {}",
            size.X,
            size.Y,
            result
        );
        Ok(())
    }

    pub fn spawn_command(&self, cmd: CommandBuilder) -> anyhow::Result<WinChild> {
        let job = create_pane_job()?;
        let mut si = conpty_startup_info();

        let mut attrs = ProcThreadAttributeList::with_capacity(1)?;
        attrs.set_pty(self.con)?;
        si.lpAttributeList = attrs.as_mut_ptr();

        let mut pi: PROCESS_INFORMATION = unsafe { mem::zeroed() };

        let (mut exe, mut cmdline) = cmd.cmdline()?;
        let cmd_os = OsString::from_wide(&cmdline);

        let cwd = cmd.current_directory();

        let res = unsafe {
            CreateProcessW(
                exe.as_mut_slice().as_mut_ptr(),
                cmdline.as_mut_slice().as_mut_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
                0,
                // Suspension closes the race where a provider descendant escapes its pane job.
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED,
                cmd.environment_block().as_mut_slice().as_mut_ptr() as *mut _,
                cwd.as_ref()
                    .map(|c| c.as_slice().as_ptr())
                    .unwrap_or(ptr::null()),
                &mut si.StartupInfo,
                &mut pi,
            )
        };
        if res == 0 {
            let err = IoError::last_os_error();
            let msg = format!(
                "CreateProcessW `{:?}` in cwd `{:?}` failed: {}",
                cmd_os,
                cwd.as_ref().map(|c| OsString::from_wide(c)),
                err
            );
            log::error!("{}", msg);
            bail!("{}", msg);
        }

        // Make sure we close out the thread handle so we don't leak it; we do this by making it owned
        let _main_thread = unsafe { OwnedHandle::from_raw_handle(pi.hThread as _) };
        let proc = unsafe { OwnedHandle::from_raw_handle(pi.hProcess as _) };

        if unsafe { AssignProcessToJobObject(job.as_raw_handle() as _, proc.as_raw_handle() as _) }
            == 0
        {
            let err = IoError::last_os_error();
            unsafe { TerminateProcess(proc.as_raw_handle() as _, 1) };
            bail!(
                "AssignProcessToJobObject failed for `{:?}`: {}",
                cmd_os,
                err
            );
        }
        if unsafe { ResumeThread(_main_thread.as_raw_handle() as _) } == u32::MAX {
            let err = IoError::last_os_error();
            unsafe { TerminateProcess(proc.as_raw_handle() as _, 1) };
            bail!("ResumeThread failed for `{:?}`: {}", cmd_os, err);
        }

        Ok(WinChild::new(proc, job))
    }
}

fn conpty_startup_info() -> STARTUPINFOEXW {
    let mut si: STARTUPINFOEXW = unsafe { mem::zeroed() };
    si.StartupInfo.cb = mem::size_of::<STARTUPINFOEXW>() as u32;
    si.StartupInfo.dwFlags = STARTF_USESTDHANDLES | STARTF_USESHOWWINDOW;
    si.StartupInfo.wShowWindow = SW_HIDE as u16;
    si.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
    si.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
    si.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
    si
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conpty_child_does_not_inherit_daemon_stdio() {
        let startup = conpty_startup_info();
        assert_ne!(startup.StartupInfo.dwFlags & STARTF_USESTDHANDLES, 0);
        assert_eq!(startup.StartupInfo.hStdInput, INVALID_HANDLE_VALUE);
        assert_eq!(startup.StartupInfo.hStdOutput, INVALID_HANDLE_VALUE);
        assert_eq!(startup.StartupInfo.hStdError, INVALID_HANDLE_VALUE);
    }
}

fn create_pane_job() -> anyhow::Result<OwnedHandle> {
    let raw = unsafe { CreateJobObjectW(ptr::null_mut(), ptr::null()) };
    if raw.is_null() {
        bail!("CreateJobObjectW failed: {}", IoError::last_os_error());
    }
    let job = unsafe { OwnedHandle::from_raw_handle(raw as _) };
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let configured = unsafe {
        SetInformationJobObject(
            job.as_raw_handle() as _,
            JobObjectExtendedLimitInformation,
            &mut limits as *mut _ as *mut _,
            mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if configured == 0 {
        bail!(
            "SetInformationJobObject failed: {}",
            IoError::last_os_error()
        );
    }
    Ok(job)
}
