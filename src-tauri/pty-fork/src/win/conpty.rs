use crate::cmdbuilder::CommandBuilder;
use crate::win::psuedocon::PsuedoCon;
use crate::{Child, MasterPty, PtyPair, PtySize, PtySystem, SlavePty};
use anyhow::Error;
use filedescriptor::{FileDescriptor, Pipe};
use std::os::windows::io::FromRawHandle;
use std::sync::{Arc, Mutex};
use winapi::um::wincon::COORD;

/// Create a pipe with a tiny buffer to force ConPTY to flush output immediately
/// instead of buffering. Default CreatePipe buffer causes ConPTY to hold data.
fn create_small_buffer_pipe() -> anyhow::Result<Pipe> {
    use std::ptr;
    use winapi::um::namedpipeapi::CreatePipe;
    use winapi::um::minwinbase::SECURITY_ATTRIBUTES;
    use winapi::um::winnt::HANDLE;

    let mut sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: ptr::null_mut(),
        bInheritHandle: 1, // TRUE — inheritable
    };

    let mut read: HANDLE = ptr::null_mut();
    let mut write: HANDLE = ptr::null_mut();

    // Buffer size = 1 forces immediate flush
    if unsafe { CreatePipe(&mut read, &mut write, &mut sa, 1) } == 0 {
        anyhow::bail!("CreatePipe failed: {}", std::io::Error::last_os_error());
    }

    Ok(Pipe {
        read: unsafe { FileDescriptor::from_raw_handle(read as _) },
        write: unsafe { FileDescriptor::from_raw_handle(write as _) },
    })
}

#[derive(Default)]
pub struct ConPtySystem {}

impl PtySystem for ConPtySystem {
    fn openpty(&self, size: PtySize) -> anyhow::Result<PtyPair> {
        let stdin = Pipe::new()?;
        // Use pipe with small buffer to force immediate flush from ConPTY
        let stdout = create_small_buffer_pipe()?;

        let con = PsuedoCon::new(
            COORD {
                X: size.cols as i16,
                Y: size.rows as i16,
            },
            stdin.read,
            stdout.write,
        )?;

        let master = ConPtyMasterPty {
            inner: Arc::new(Mutex::new(Inner {
                con,
                readable: stdout.read,
                writable: Some(stdin.write),
                size,
            })),
        };

        let slave = ConPtySlavePty {
            inner: master.inner.clone(),
        };

        Ok(PtyPair {
            master: Box::new(master),
            slave: Box::new(slave),
        })
    }
}

struct Inner {
    con: PsuedoCon,
    readable: FileDescriptor,
    writable: Option<FileDescriptor>,
    size: PtySize,
}

impl Inner {
    pub fn resize(
        &mut self,
        num_rows: u16,
        num_cols: u16,
        pixel_width: u16,
        pixel_height: u16,
    ) -> Result<(), Error> {
        self.con.resize(COORD {
            X: num_cols as i16,
            Y: num_rows as i16,
        })?;
        self.size = PtySize {
            rows: num_rows,
            cols: num_cols,
            pixel_width,
            pixel_height,
        };
        Ok(())
    }
}

#[derive(Clone)]
pub struct ConPtyMasterPty {
    inner: Arc<Mutex<Inner>>,
}

pub struct ConPtySlavePty {
    inner: Arc<Mutex<Inner>>,
}

impl MasterPty for ConPtyMasterPty {
    fn resize(&self, size: PtySize) -> anyhow::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.resize(size.rows, size.cols, size.pixel_width, size.pixel_height)
    }

    fn get_size(&self) -> Result<PtySize, Error> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.size.clone())
    }

    fn try_clone_reader(&self) -> anyhow::Result<Box<dyn std::io::Read + Send>> {
        // WORKAROUND: On Windows 11 Insider builds (26200+), DuplicateHandle
        // on ConPTY pipe handles creates non-functional read handles.
        // Solution: return the ORIGINAL handle and put the clone in inner
        // (inner doesn't read, so broken clone is fine there).
        let mut inner = self.inner.lock().unwrap();
        let cloned = inner.readable.try_clone()?;
        let original = std::mem::replace(&mut inner.readable, cloned);
        Ok(Box::new(original))
    }

    fn take_writer(&self) -> anyhow::Result<Box<dyn std::io::Write + Send>> {
        Ok(Box::new(
            self.inner
                .lock()
                .unwrap()
                .writable
                .take()
                .ok_or_else(|| anyhow::anyhow!("writer already taken"))?,
        ))
    }
}

impl SlavePty for ConPtySlavePty {
    fn spawn_command(&self, cmd: CommandBuilder) -> anyhow::Result<Box<dyn Child + Send + Sync>> {
        let inner = self.inner.lock().unwrap();
        let child = inner.con.spawn_command(cmd)?;
        Ok(Box::new(child))
    }
}

