use std::{
    ffi::OsStr,
    fs::File,
    io,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle},
        process::ExitStatusExt,
    },
    process::{Command, ExitStatus},
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{
        GetLastError, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, WAIT_OBJECT_0,
        WAIT_TIMEOUT,
    },
    System::{
        Pipes::CreatePipe,
        Threading::{
            CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
            InitializeProcThreadAttributeList, TerminateProcess, UpdateProcThreadAttribute,
            WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
            EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
            STARTF_USESTDHANDLES, STARTUPINFOEXW,
        },
    },
};

pub(super) struct Child {
    process: OwnedHandle,
    id: u32,
    pub(super) stdin: Option<File>,
    pub(super) stdout: Option<File>,
    #[cfg(test)]
    pub(super) stderr: Option<File>,
}

impl AsRawHandle for Child {
    fn as_raw_handle(&self) -> RawHandle {
        self.process.as_raw_handle()
    }
}

impl crate::provider_update::ProcessChild for Child {
    fn id(&self) -> u32 {
        self.id
    }
    fn kill(&mut self) -> io::Result<()> {
        Child::kill(self)
    }
    fn process_handle(&self) -> HANDLE {
        self.as_raw_handle().cast()
    }
}

impl Child {
    pub(super) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        match unsafe { WaitForSingleObject(self.as_raw_handle().cast(), 0) } {
            WAIT_OBJECT_0 => self.status().map(Some),
            WAIT_TIMEOUT => Ok(None),
            _ => Err(io::Error::last_os_error()),
        }
    }
    pub(super) fn wait(&mut self) -> io::Result<ExitStatus> {
        if unsafe { WaitForSingleObject(self.as_raw_handle().cast(), u32::MAX) } != WAIT_OBJECT_0 {
            return Err(io::Error::last_os_error());
        }
        self.status()
    }
    fn status(&self) -> io::Result<ExitStatus> {
        let mut code = 0;
        if unsafe { GetExitCodeProcess(self.as_raw_handle().cast(), &mut code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(ExitStatus::from_raw(code))
    }
    pub(super) fn kill(&mut self) -> io::Result<()> {
        if unsafe { TerminateProcess(self.as_raw_handle().cast(), 1) } != 0 {
            Ok(())
        } else {
            let error = io::Error::last_os_error();
            if self.try_wait()?.is_some() {
                Ok(())
            } else {
                Err(error)
            }
        }
    }
}

struct Attributes {
    storage: Vec<usize>,
}
impl Attributes {
    fn new(handles: &mut [HANDLE; 3]) -> io::Result<Self> {
        let mut size = 0;
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut size);
        }
        if size == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut storage = vec![0; size.div_ceil(std::mem::size_of::<usize>())];
        let attributes = storage.as_mut_ptr().cast();
        if unsafe { InitializeProcThreadAttributeList(attributes, 1, 0, &mut size) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let result = Self { storage };
        if unsafe {
            UpdateProcThreadAttribute(
                attributes,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_mut_ptr().cast(),
                std::mem::size_of_val(handles),
                null_mut(),
                null(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(result)
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.storage.as_mut_ptr().cast());
        }
    }
}

fn pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let mut reader = null_mut();
    let mut writer = null_mut();
    if unsafe { CreatePipe(&mut reader, &mut writer, null(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe {
        (
            OwnedHandle::from_raw_handle(reader.cast()),
            OwnedHandle::from_raw_handle(writer.cast()),
        )
    })
}

fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let units: Vec<_> = value.encode_wide().collect();
    if units.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "NUL in process argument",
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
        let count = if unit == b'"' as u16 {
            slashes * 2 + 1
        } else {
            slashes
        };
        output.extend(std::iter::repeat_n(b'\\' as u16, count));
        output.push(unit);
        slashes = 0;
    }
    output.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    output.push(b'"' as u16);
    Ok(())
}

pub(super) fn spawn(command: &Command) -> io::Result<Child> {
    let mut program = wide(command.get_program())?;
    program.push(0);
    let mut command_line = Vec::new();
    argument(command.get_program(), &mut command_line)?;
    for arg in command.get_args() {
        command_line.push(b' ' as u16);
        argument(arg, &mut command_line)?;
    }
    command_line.push(0);
    let mut environment = Vec::new();
    for (key, value) in command.get_envs() {
        if let Some(value) = value {
            environment.extend(wide(key)?);
            environment.push(b'=' as u16);
            environment.extend(wide(value)?);
            environment.push(0);
        }
    }
    environment.push(0);
    if environment.len() == 1 {
        environment.push(0);
    }
    let cwd = command
        .get_current_dir()
        .map(|path| wide(path.as_os_str()))
        .transpose()?;
    let cwd = cwd.map(|mut path| {
        path.push(0);
        path
    });
    let (input, stdin) = pipe()?;
    let (stdout, output) = pipe()?;
    #[cfg(test)]
    let (stderr, error) = {
        let (reader, writer) = pipe()?;
        (reader, writer)
    };
    #[cfg(not(test))]
    let error: OwnedHandle = File::options().write(true).open(r"\\.\NUL")?.into();
    let child_handles = [input, output, error];
    let mut handles = child_handles
        .each_ref()
        .map(|handle| handle.as_raw_handle().cast());
    let mut attributes = Attributes::new(&mut handles)?;
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = handles[0];
    startup.StartupInfo.hStdOutput = handles[1];
    startup.StartupInfo.hStdError = handles[2];
    startup.lpAttributeList = attributes.storage.as_mut_ptr().cast();
    let mut information = PROCESS_INFORMATION::default();
    // Only the listed child ends are inheritable, and only through process creation.
    for handle in handles {
        if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) } == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    let created = unsafe {
        CreateProcessW(
            program.as_ptr(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            1,
            CREATE_SUSPENDED
                | CREATE_NO_WINDOW
                | CREATE_UNICODE_ENVIRONMENT
                | EXTENDED_STARTUPINFO_PRESENT,
            environment.as_ptr().cast(),
            cwd.as_ref().map_or(null(), |path| path.as_ptr()),
            &startup.StartupInfo,
            &mut information,
        )
    };
    let error = unsafe { GetLastError() };
    drop(child_handles);
    if created == 0 {
        return Err(io::Error::from_raw_os_error(error as i32));
    }
    let process = unsafe { OwnedHandle::from_raw_handle(information.hProcess.cast()) };
    let thread = unsafe { OwnedHandle::from_raw_handle(information.hThread.cast()) };
    drop(thread);
    Ok(Child {
        process,
        id: information.dwProcessId,
        stdin: Some(File::from(stdin)),
        stdout: Some(File::from(stdout)),
        #[cfg(test)]
        stderr: Some(File::from(stderr)),
    })
}
