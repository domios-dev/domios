use std::ffi::OsString;
use std::io;

#[cfg(windows)]
pub fn run(args: impl IntoIterator<Item = OsString>) -> io::Result<i32> {
    use windows_sys::Win32::System::Console::{
        SetConsoleCtrlHandler, CTRL_BREAK_EVENT, CTRL_C_EVENT,
    };
    unsafe extern "system" fn protect_wrapper(event: u32) -> i32 {
        i32::from(event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT)
    }
    // A registered handler is process-local; the NULL ignore flag is inherited by children.
    if unsafe { SetConsoleCtrlHandler(Some(protect_wrapper), 1) } == 0
        || unsafe { SetConsoleCtrlHandler(None, 0) } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut args = args.into_iter();
    let executable = args
        .next()
        .ok_or_else(|| io::Error::other("PTY child executable missing"))?;
    let status = std::process::Command::new(executable).args(args).status()?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(not(windows))]
pub fn run(_: impl IntoIterator<Item = OsString>) -> io::Result<i32> {
    Err(io::Error::other("PTY child launch is Windows-only"))
}
