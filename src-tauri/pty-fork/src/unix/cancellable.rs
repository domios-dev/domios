use filedescriptor::FileDescriptor;
use std::io::{self, Write};
use std::net::Shutdown;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::Arc;

fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "PTY writer cancelled")
}

fn wait(fd: i32, events: i16, cancel: Option<i32>) -> io::Result<()> {
    let mut descriptors = [
        libc::pollfd {
            fd,
            events,
            revents: 0,
        },
        libc::pollfd {
            fd: cancel.unwrap_or(-1),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    loop {
        let result = unsafe { libc::poll(descriptors.as_mut_ptr(), descriptors.len() as _, -1) };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if descriptors[1].revents != 0 {
            return Err(cancelled());
        }
        if descriptors[0].revents != 0 {
            return Ok(());
        }
    }
}

pub(super) fn wait_readable(fd: i32) -> io::Result<()> {
    wait(fd, libc::POLLIN, None)
}

pub(super) fn new(fd: FileDescriptor) -> io::Result<crate::CancellableWriter> {
    let (writer, cancel_write) = native_writer(fd)?;
    Ok(crate::CancellableWriter {
        writer: Box::new(writer),
        cancel: Arc::new(move || {
            let _ = cancel_write.shutdown(Shutdown::Write);
        }),
    })
}

fn native_writer(fd: FileDescriptor) -> io::Result<(Writer, UnixStream)> {
    let (cancel_read, cancel_write) = UnixStream::pair()?;
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok((Writer { fd, cancel_read }, cancel_write))
}

struct Writer {
    fd: FileDescriptor,
    cancel_read: UnixStream,
}

impl Writer {
    fn write_with_wait_hook(
        &mut self,
        bytes: &[u8],
        mut waiting: impl FnMut(),
    ) -> io::Result<usize> {
        loop {
            let mut signal = libc::pollfd {
                fd: self.cancel_read.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            if unsafe { libc::poll(&mut signal, 1, 0) } > 0 {
                return Err(cancelled());
            }
            match self.fd.write(bytes) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    waiting();
                    wait(
                        self.fd.as_raw_fd(),
                        libc::POLLOUT,
                        Some(self.cancel_read.as_raw_fd()),
                    )?;
                }
                result => return result,
            }
        }
    }
}

impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.write_with_wait_hook(bytes, || {})
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
