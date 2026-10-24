use super::*;
use std::{
    fs::File,
    io::Read,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::PathBuf,
    ptr::{null, null_mut},
};
use windows_sys::Win32::System::{Console::*, Pipes::*};

const MARKER: &str = "fixture-tree:";

fn pipe() -> (OwnedHandle, OwnedHandle) {
    let mut read = null_mut();
    let mut write = null_mut();
    assert_ne!(unsafe { CreatePipe(&mut read, &mut write, null(), 0) }, 0);
    unsafe {
        (
            OwnedHandle::from_raw_handle(read),
            OwnedHandle::from_raw_handle(write),
        )
    }
}

fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

// Packaged activation starts the shell outside the launcher's Job; its children escape both Jobs.
fn packaged_shell() -> PathBuf {
    let alias = PathBuf::from(std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA"))
        .join("Microsoft/WindowsApps/pwsh.exe");
    assert!(
        std::fs::symlink_metadata(&alias).is_ok(),
        "external ConPTY fixture requires the packaged PowerShell alias {}",
        alias.display()
    );
    alias
}

fn nested_job() -> OwnedHandle {
    let job = unsafe { CreateJobObjectW(null(), null()) };
    assert!(!job.is_null());
    let job = unsafe { OwnedHandle::from_raw_handle(job) };
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags =
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
    assert_ne!(
        unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        },
        0
    );
    job
}

fn in_job(member: &Member, job: &OwnedHandle) -> bool {
    let mut contained = 0;
    assert_ne!(
        unsafe { IsProcessInJob(member.handle as HANDLE, job.as_raw_handle(), &mut contained) },
        0
    );
    contained != 0
}

fn commands(output: OwnedHandle) -> Vec<u32> {
    let mut output = File::from(output);
    let mut seen = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = output.read(&mut chunk).unwrap();
        assert_ne!(
            read, 0,
            "ConPTY fixture exited before command acknowledgement"
        );
        seen.extend_from_slice(&chunk[..read]);
        let text = String::from_utf8_lossy(&seen);
        if let Some(start) = text.find(MARKER) {
            let json = &text[start + MARKER.len()..];
            if let Some(end) = json.find(']') {
                return serde_json::from_str(&json[..=end]).unwrap();
            }
        }
    }
}

pub(super) fn launch() {
    let job = nested_job();
    let (input_read, _input_write) = pipe();
    let (output_read, output_write) = pipe();
    let mut console = 0;
    assert_eq!(
        unsafe {
            CreatePseudoConsole(
                COORD { X: 200, Y: 30 },
                input_read.as_raw_handle(),
                output_write.as_raw_handle(),
                0,
                &mut console,
            )
        },
        0
    );
    let mut bytes = 0;
    unsafe { InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut bytes) };
    let mut storage = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
    let attrs = storage.as_mut_ptr().cast();
    assert_ne!(
        unsafe { InitializeProcThreadAttributeList(attrs, 1, 0, &mut bytes) },
        0
    );
    assert_ne!(
        unsafe {
            UpdateProcThreadAttribute(
                attrs,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                console as *const _,
                std::mem::size_of_val(&console),
                null_mut(),
                null(),
            )
        },
        0
    );
    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = std::mem::size_of_val(&startup) as u32;
    // Without explicit invalid std handles the shell inherits the helper's pipes instead of ConPTY.
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
    startup.lpAttributeList = attrs;
    let shell = packaged_shell();
    let script = crate::test_support::fixture_manifest_dir()
        .join("../../../tests/fixtures/agent-runtime/escaped-tree.mjs")
        .canonicalize()
        .unwrap();
    let script = script.to_str().unwrap();
    let script = script.strip_prefix(r"\\?\").unwrap_or(script);
    let command = format!(
        "\"{}\" -NoLogo -NoProfile -NonInteractive -Command node '{script}'",
        shell.display()
    );
    let application = wide(shell.as_os_str());
    let mut command = wide(std::ffi::OsStr::new(&command));
    let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command.as_mut_ptr(),
            null(),
            null(),
            0,
            EXTENDED_STARTUPINFO_PRESENT,
            null(),
            null(),
            &startup.StartupInfo,
            &mut process,
        )
    };
    unsafe { DeleteProcThreadAttributeList(attrs) };
    assert_ne!(
        created,
        0,
        "CreateProcessW: {}",
        std::io::Error::last_os_error()
    );
    let process_handle = unsafe { OwnedHandle::from_raw_handle(process.hProcess) };
    let _thread = unsafe { OwnedHandle::from_raw_handle(process.hThread) };
    assert_ne!(
        unsafe { AssignProcessToJobObject(job.as_raw_handle(), process_handle.as_raw_handle()) },
        0,
        "AssignProcessToJobObject: {}",
        std::io::Error::last_os_error()
    );
    drop(input_read);
    drop(output_write);
    let commands = commands(output_read);
    for pid in &commands {
        let member = Member::retain(*pid).unwrap().unwrap();
        assert!(!in_job(&member, &job), "command must escape the shell Job");
    }
    println!(
        "fixture-pids:{}",
        serde_json::json!({"shell": process.dwProcessId, "commands": commands})
    );
    std::io::stdout().flush().unwrap();
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
    unsafe {
        TerminateJobObject(job.as_raw_handle(), 1);
        ClosePseudoConsole(console);
    }
}
