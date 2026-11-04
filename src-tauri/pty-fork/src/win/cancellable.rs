use filedescriptor::FileDescriptor;
use std::io::{self, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use winapi::shared::sddl::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use winapi::shared::winerror::{ERROR_IO_PENDING, ERROR_PIPE_CONNECTED};
use winapi::um::fileapi::{CreateFileW, WriteFile, OPEN_EXISTING};
use winapi::um::handleapi::INVALID_HANDLE_VALUE;
use winapi::um::ioapiset::{CancelIoEx, GetOverlappedResult};
use winapi::um::minwinbase::{OVERLAPPED, SECURITY_ATTRIBUTES};
use winapi::um::namedpipeapi::{ConnectNamedPipe, CreateNamedPipeW};
use winapi::um::synchapi::{CreateEventW, SetEvent, WaitForMultipleObjects, WaitForSingleObject};
use winapi::um::winbase::{
    LocalFree, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, INFINITE, PIPE_ACCESS_OUTBOUND,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT, WAIT_OBJECT_0,
};
use winapi::um::winnt::GENERIC_READ;

fn event() -> io::Result<OwnedHandle> {
    let handle = unsafe { CreateEventW(ptr::null_mut(), 1, 0, ptr::null()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(handle as _) })
}

pub(super) fn pipe() -> io::Result<(FileDescriptor, crate::CancellableWriter)> {
    let (input, writer) = native_pipe()?;
    let cancellation = writer.cancelled.clone();
    Ok((
        input,
        crate::CancellableWriter {
            writer: Box::new(writer),
            cancel: Arc::new(move || unsafe {
                SetEvent(cancellation.as_raw_handle() as _);
            }),
        },
    ))
}

fn native_pipe() -> io::Result<(FileDescriptor, Writer)> {
    static NEXT_PIPE: AtomicU64 = AtomicU64::new(0);
    let name: Vec<u16> = format!(
        "\\\\.\\pipe\\domios-pty-input-{}-{}\0",
        std::process::id(),
        NEXT_PIPE.fetch_add(1, Ordering::Relaxed)
    )
    .encode_utf16()
    .collect();
    let sddl: Vec<u16> = "D:P(A;;GA;;;OW)\0".encode_utf16().collect();
    let mut descriptor = ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_OUTBOUND | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            65536,
            0,
            0,
            &mut security,
        )
    };
    let error = io::Error::last_os_error();
    unsafe {
        LocalFree(descriptor as _);
    }
    if handle == INVALID_HANDLE_VALUE {
        return Err(error);
    }
    let output = unsafe { OwnedHandle::from_raw_handle(handle as _) };
    let input = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ,
            0,
            ptr::null_mut(),
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        )
    };
    if input == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let input = unsafe { FileDescriptor::from_raw_handle(input as _) };
    let mut connect: OVERLAPPED = unsafe { std::mem::zeroed() };
    let connected = unsafe { ConnectNamedPipe(output.as_raw_handle() as _, &mut connect) };
    if connected == 0
        && io::Error::last_os_error().raw_os_error() != Some(ERROR_PIPE_CONNECTED as i32)
    {
        return Err(io::Error::last_os_error());
    }
    let cancelled = Arc::new(event()?);
    Ok((input, Writer { output, cancelled }))
}

struct Writer {
    output: OwnedHandle,
    cancelled: Arc<OwnedHandle>,
}

struct PendingOperation<'a> {
    handle: winapi::um::winnt::HANDLE,
    operation: &'a mut OVERLAPPED,
}

impl PendingOperation<'_> {
    fn result(&mut self) -> io::Result<u32> {
        let mut written = 0;
        if unsafe { GetOverlappedResult(self.handle, self.operation, &mut written, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(written)
    }
}

impl Drop for PendingOperation<'_> {
    fn drop(&mut self) {
        let mut written = 0;
        unsafe {
            CancelIoEx(self.handle, self.operation);
            // The kernel must release the borrowed buffer, including during unwinding.
            GetOverlappedResult(self.handle, self.operation, &mut written, 1);
        }
    }
}

impl Writer {
    fn write_with_pending_hook(
        &mut self,
        bytes: &[u8],
        mut pending: impl FnMut(),
    ) -> io::Result<usize> {
        let cancelled = self.cancelled.as_raw_handle() as _;
        if unsafe { WaitForSingleObject(cancelled, 0) } == WAIT_OBJECT_0 {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "PTY writer cancelled",
            ));
        }
        let completed = event()?;
        let mut operation: OVERLAPPED = unsafe { std::mem::zeroed() };
        operation.hEvent = completed.as_raw_handle() as _;
        let handle = self.output.as_raw_handle() as _;
        let mut written = 0;
        let result = unsafe {
            WriteFile(
                handle,
                bytes.as_ptr() as _,
                bytes.len().min(u32::MAX as usize) as u32,
                &mut written,
                &mut operation,
            )
        };
        if result == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
                return Err(error);
            }
            let mut operation = PendingOperation {
                handle,
                operation: &mut operation,
            };
            pending();
            let handles = [cancelled, completed.as_raw_handle() as _];
            let selected = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
            if selected != WAIT_OBJECT_0 + 1 {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "PTY writer cancelled",
                ));
            }
            written = operation.result()?;
        }
        Ok(written as usize)
    }
}

impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.write_with_pending_hook(bytes, || {})
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
