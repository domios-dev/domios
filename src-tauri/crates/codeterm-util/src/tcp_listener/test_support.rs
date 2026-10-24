use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{
        CreateEventW, CreateProcessW, SetEvent, TerminateProcess, WaitForSingleObject,
        CREATE_NO_WINDOW, PROCESS_INFORMATION, STARTUPINFOW,
    },
};

pub struct InheritingChild {
    process: OwnedHandle,
    release: OwnedHandle,
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn event(name: &str) -> OwnedHandle {
    let name = wide(name);
    let raw = unsafe { CreateEventW(std::ptr::null(), 1, 0, name.as_ptr()) };
    assert!(
        !raw.is_null(),
        "create child event: {}",
        std::io::Error::last_os_error()
    );
    unsafe { OwnedHandle::from_raw_handle(raw) }
}

impl InheritingChild {
    pub fn start() -> Self {
        let id = uuid::Uuid::new_v4();
        let ready_name = format!("Local\\domios-listener-ready-{id}");
        let release_name = format!("Local\\domios-listener-release-{id}");
        let ready = event(&ready_name);
        let release = event(&release_name);
        let executable =
            std::path::PathBuf::from(std::env::var_os("SystemRoot").expect("Windows root"))
                .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let mut command = wide(&format!(
            "\"{}\" -NoLogo -NoProfile -NonInteractive -Command \"[Threading.EventWaitHandle]::OpenExisting('{ready_name}').Set(); [Threading.EventWaitHandle]::OpenExisting('{release_name}').WaitOne() | Out-Null\"",
            executable.display()
        ));
        let startup = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOW>() as u32,
            ..unsafe { std::mem::zeroed() }
        };
        let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        let created = unsafe {
            CreateProcessW(
                std::ptr::null(),
                command.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
                CREATE_NO_WINDOW,
                std::ptr::null(),
                std::ptr::null(),
                &startup,
                &mut process,
            )
        };
        assert_ne!(
            created,
            0,
            "spawn inheriting child: {}",
            std::io::Error::last_os_error()
        );
        let _thread = unsafe { OwnedHandle::from_raw_handle(process.hThread) };
        let child = Self {
            process: unsafe { OwnedHandle::from_raw_handle(process.hProcess) },
            release,
        };
        assert_eq!(
            unsafe { WaitForSingleObject(ready.as_raw_handle(), 10_000) },
            WAIT_OBJECT_0,
            "child must acknowledge readiness"
        );
        assert!(child.is_alive());
        child
    }

    pub fn is_alive(&self) -> bool {
        unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) == WAIT_TIMEOUT }
    }
}

impl Drop for InheritingChild {
    fn drop(&mut self) {
        unsafe {
            SetEvent(self.release.as_raw_handle());
            if WaitForSingleObject(self.process.as_raw_handle(), 10_000) != WAIT_OBJECT_0 {
                TerminateProcess(self.process.as_raw_handle(), 1);
                WaitForSingleObject(self.process.as_raw_handle(), 10_000);
            }
        }
    }
}
