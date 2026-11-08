use super::*;
use std::{
    ffi::OsStr,
    fs::File,
    io::{self, PipeReader},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::ExitStatusExt,
    },
    process::{Command, ExitStatus},
    ptr::{null, null_mut},
    time::Duration,
};

pub struct ProbeChild {
    process: OwnedHandle,
    job: Option<Job>,
    pub stdout: Option<PipeReader>,
}

struct Attributes(Vec<usize>);

impl Attributes {
    fn new(handles: &mut [HANDLE; 3], jobs: &mut [HANDLE; 1]) -> io::Result<Self> {
        let mut size = 0;
        unsafe { InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut size) };
        if size == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut storage = vec![0; size.div_ceil(std::mem::size_of::<usize>())];
        let attributes = storage.as_mut_ptr().cast();
        if unsafe { InitializeProcThreadAttributeList(attributes, 2, 0, &mut size) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let owned = Self(storage);
        for (attribute, pointer, bytes) in [
            (
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                handles.as_mut_ptr().cast(),
                std::mem::size_of_val(handles),
            ),
            (
                PROC_THREAD_ATTRIBUTE_JOB_LIST,
                jobs.as_mut_ptr().cast(),
                std::mem::size_of_val(jobs),
            ),
        ] {
            if unsafe {
                UpdateProcThreadAttribute(
                    attributes,
                    0,
                    attribute as usize,
                    pointer,
                    bytes,
                    null_mut(),
                    null(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(owned)
    }
}

impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.0.as_mut_ptr().cast()) };
    }
}

fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let units: Vec<_> = value.encode_wide().collect();
    if units.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "NUL in command",
        ));
    }
    Ok(units)
}

fn argument(value: &OsStr, output: &mut Vec<u16>) -> io::Result<()> {
    output.push(b'"' as u16);
    let mut slashes = 0;
    for unit in wide(value)? {
        if unit == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        output.extend(std::iter::repeat_n(
            b'\\' as u16,
            if unit == b'"' as u16 {
                slashes * 2 + 1
            } else {
                slashes
            },
        ));
        output.push(unit);
        slashes = 0;
    }
    output.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    output.push(b'"' as u16);
    Ok(())
}

impl ProbeChild {
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        if command.get_program().is_empty() || command.get_envs().next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid probe command",
            ));
        }
        let binary = std::path::Path::new(command.get_program());
        let batch = binary
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"));
        let program = if batch {
            std::env::var_os("SystemRoot")
                .map(|root| std::path::PathBuf::from(root).join("System32/cmd.exe"))
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "SystemRoot is missing"))?
        } else {
            binary.to_path_buf()
        };
        let mut line = Vec::new();
        argument(program.as_os_str(), &mut line)?;
        if batch {
            if wide(binary.as_os_str())?
                .iter()
                .any(|unit| [b'%' as u16, b'"' as u16, 10, 13].contains(unit))
                || command.get_args().any(|arg| arg != "--version")
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Unsafe batch probe command",
                ));
            }
            line.extend(" /d /v:off /s /c \"".encode_utf16());
            argument(binary.as_os_str(), &mut line)?;
        }
        for arg in command.get_args() {
            line.push(b' ' as u16);
            argument(arg, &mut line)?;
        }
        if batch {
            line.push(b'"' as u16);
        }
        line.push(0);
        let mut program = wide(program.as_os_str())?;
        program.push(0);
        let cwd = command
            .get_current_dir()
            .map(|path| {
                let mut path = wide(path.as_os_str())?;
                path.push(0);
                Ok::<_, io::Error>(path)
            })
            .transpose()?;
        let job = Job::new().map_err(io::Error::other)?;
        let (stdout, writer) = io::pipe()?;
        let input = File::open(r"\\.\NUL")?;
        let error = File::options().write(true).open(r"\\.\NUL")?;
        let mut handles = [
            input.as_raw_handle(),
            writer.as_raw_handle(),
            error.as_raw_handle(),
        ];
        for handle in handles {
            if unsafe {
                SetHandleInformation(handle.cast(), HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT)
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        let mut jobs = [job.0 as HANDLE];
        let mut attributes = Attributes::new(&mut handles, &mut jobs)?;
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = handles[0];
        startup.StartupInfo.hStdOutput = handles[1];
        startup.StartupInfo.hStdError = handles[2];
        startup.lpAttributeList = attributes.0.as_mut_ptr().cast();
        let mut information = PROCESS_INFORMATION::default();
        // Job assignment is atomic with creation, including owner death before this call returns.
        if unsafe {
            CreateProcessW(
                program.as_ptr(),
                line.as_mut_ptr(),
                null(),
                null(),
                1,
                CREATE_NO_WINDOW | CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT,
                null(),
                cwd.as_ref().map_or(null(), |path| path.as_ptr()),
                &startup.StartupInfo,
                &mut information,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let process = unsafe { OwnedHandle::from_raw_handle(information.hProcess) };
        let thread = unsafe { OwnedHandle::from_raw_handle(information.hThread) };
        let child = Self {
            process,
            job: Some(job),
            stdout: Some(stdout),
        };
        if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
            return Err(io::Error::last_os_error());
        }
        Ok(child)
    }

    pub fn wait_timeout(&mut self, timeout: Duration) -> io::Result<Option<ExitStatus>> {
        let millis = timeout.as_millis().min((u32::MAX - 1) as u128) as u32;
        match unsafe { WaitForSingleObject(self.process.as_raw_handle(), millis) } {
            WAIT_OBJECT_0 => {
                let mut code = 0;
                if unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &mut code) } == 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(Some(ExitStatus::from_raw(code)))
            }
            WAIT_TIMEOUT => Ok(None),
            _ => Err(io::Error::last_os_error()),
        }
    }

    pub fn terminate(&mut self) {
        if let Some(job) = &self.job {
            if job.try_terminate().is_err() {
                unsafe { TerminateProcess(self.process.as_raw_handle(), 1) };
            }
        }
        unsafe { WaitForSingleObject(self.process.as_raw_handle(), INFINITE) };
    }
}

impl Drop for ProbeChild {
    fn drop(&mut self) {
        drop(self.job.take());
        unsafe { WaitForSingleObject(self.process.as_raw_handle(), INFINITE) };
    }
}

#[cfg(test)]
mod tests;
