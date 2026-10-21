use super::{last_error, wide_name, OwnedHandle};
use windows_sys::Win32::Foundation::ERROR_NO_MORE_FILES;
use windows_sys::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, MODULEENTRY32W, TH32CS_SNAPMODULE,
};
use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::System::WindowsProgramming::GetUserNameW;

pub(in crate::pty) fn print_pty_environment(shell: &str, child: Option<u32>) {
    let pid = std::process::id();
    eprintln!(
        "PTY environment: pid={pid}, session={:?}, user={:?}, shell={shell:?}, child={child:?}, child_session={:?}, modules={:?}",
        session_id(pid),
        username(),
        child.map(session_id),
        console_modules(),
    );
    match super::conpty_host_children_of_current_process() {
        Ok(hosts) => {
            for host in hosts {
                let image = process_image(host);
                eprintln!(
                    "PTY host: pid={host}, session={:?}, image={image:?}, version={:?}",
                    session_id(host),
                    image.as_deref().map(file_version),
                );
            }
        }
        Err(error) => eprintln!("PTY hosts: observation failed: {error}"),
    }
}

fn session_id(pid: u32) -> Result<u32, String> {
    let mut session = 0;
    if unsafe { ProcessIdToSessionId(pid, &mut session) } == 0 {
        return Err(last_error("ProcessIdToSessionId"));
    }
    Ok(session)
}

fn username() -> Result<String, String> {
    let mut name = [0u16; 257];
    let mut length = name.len() as u32;
    if unsafe { GetUserNameW(name.as_mut_ptr(), &mut length) } == 0 {
        return Err(last_error("GetUserNameW"));
    }
    Ok(wide_name(&name))
}

fn process_image(pid: u32) -> Result<String, String> {
    let process = OwnedHandle::new(
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) },
        "OpenProcess for console host",
    )?;
    let mut path = vec![0u16; 32768];
    let mut length = path.len() as u32;
    if unsafe { QueryFullProcessImageNameW(process.0, 0, path.as_mut_ptr(), &mut length) } == 0 {
        return Err(last_error("QueryFullProcessImageNameW"));
    }
    Ok(String::from_utf16_lossy(&path[..length as usize]))
}

fn console_modules() -> Result<Vec<(String, Result<String, String>)>, String> {
    let snapshot = OwnedHandle::new(
        unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, std::process::id()) },
        "console module snapshot",
    )?;
    let mut entry = MODULEENTRY32W::default();
    entry.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
    if unsafe { Module32FirstW(snapshot.0, &mut entry) } == 0 {
        return Err(last_error("Module32FirstW"));
    }
    let mut modules = Vec::new();
    loop {
        let name = wide_name(&entry.szModule);
        if name.eq_ignore_ascii_case("conpty.dll") || name.eq_ignore_ascii_case("kernel32.dll") {
            let path = wide_name(&entry.szExePath);
            let version = file_version(&path);
            modules.push((path, version));
        }
        if unsafe { Module32NextW(snapshot.0, &mut entry) } == 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                break;
            }
            return Err(last_error("Module32NextW"));
        }
    }
    Ok(modules)
}

fn file_version(path: &str) -> Result<String, String> {
    let path: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), std::ptr::null_mut()) };
    if size == 0 {
        return Err(last_error("GetFileVersionInfoSizeW"));
    }
    let mut data = vec![0u8; size as usize];
    if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, data.as_mut_ptr().cast()) } == 0 {
        return Err(last_error("GetFileVersionInfoW"));
    }
    let mut value = std::ptr::null_mut();
    let mut length = 0;
    let root = [b'\\' as u16, 0];
    if unsafe { VerQueryValueW(data.as_ptr().cast(), root.as_ptr(), &mut value, &mut length) } == 0
        || value.is_null()
        || (length as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>()
    {
        return Err("version resource has no fixed file info".into());
    }
    let version = unsafe { std::ptr::read_unaligned(value.cast::<VS_FIXEDFILEINFO>()) };
    Ok(format!(
        "{}.{}.{}.{}",
        version.dwFileVersionMS >> 16,
        version.dwFileVersionMS & 0xffff,
        version.dwFileVersionLS >> 16,
        version.dwFileVersionLS & 0xffff,
    ))
}
