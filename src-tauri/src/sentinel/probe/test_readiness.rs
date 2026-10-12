use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use parking_lot::Mutex;

use super::cancellation::Cancellation;
use super::process::{DeadlineReceipt, Deadlines};

pub(super) const SETUP_DEADLINE: Duration = Duration::from_secs(30);

pub(super) struct Readiness {
    address: SocketAddr,
    connection: Mutex<mpsc::Receiver<std::io::Result<TcpStream>>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Readiness {
    pub(super) fn new() -> Arc<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _ = tx.send(listener.accept().map(|(stream, _)| stream));
        });
        Arc::new(Self {
            address,
            connection: Mutex::new(rx),
            worker: Some(worker),
        })
    }

    pub(super) fn address(&self) -> SocketAddr {
        self.address
    }

    pub(super) fn signal(
        self: &Arc<Self>,
        limit: Duration,
        runtime: Duration,
    ) -> (Cancellation, mpsc::Receiver<DeadlineReceipt>) {
        let ready = self.clone();
        let (tx, rx) = mpsc::channel();
        let signal = Cancellation::with_readiness(move || {
            let mut stream = ready
                .connection
                .lock()
                .recv_timeout(SETUP_DEADLINE)
                .map_err(|error| error.to_string())?
                .map_err(|error| error.to_string())?;
            stream
                .set_read_timeout(Some(SETUP_DEADLINE))
                .map_err(|error| error.to_string())?;
            stream
                .set_write_timeout(Some(SETUP_DEADLINE))
                .map_err(|error| error.to_string())?;
            let mut marker = [0];
            stream
                .read_exact(&mut marker)
                .map_err(|error| error.to_string())?;
            if marker != [1] {
                return Err("invalid child readiness marker".into());
            }
            let deadlines = Deadlines::start(limit, runtime).map_err(|error| error.to_string())?;
            tx.send(deadlines.receipt())
                .map_err(|error| error.to_string())?;
            stream.write_all(&[2]).map_err(|error| error.to_string())?;
            Ok(deadlines)
        });
        (signal, rx)
    }

    pub(super) fn finite_command(&self, code: i32) -> String {
        let exe = std::env::current_exe().unwrap();
        let selector = "sentinel::probe::test_readiness::finite_ready_child";
        #[cfg(unix)]
        return format!("SENTINEL_FIXTURE_READY={} {} --exact --ignored --nocapture {selector} >/dev/null 2>&1 && {{ printf out; printf err >&2; exit {code}; }}", self.address, quote(&exe));
        #[cfg(windows)]
        return format!("$env:SENTINEL_FIXTURE_READY='{}'; & {} --exact --ignored --nocapture {selector} *> $null; if ($LASTEXITCODE -ne 0) {{ exit $LASTEXITCODE }}; [Console]::Out.Write('out'); [Console]::Error.Write('err'); exit {code}", self.address, quote(&exe));
    }
}

impl Drop for Readiness {
    fn drop(&mut self) {
        let _ = TcpStream::connect_timeout(&self.address, SETUP_DEADLINE);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

pub(super) fn child_ready(address: &str) {
    let mut stream = TcpStream::connect_timeout(&address.parse().unwrap(), SETUP_DEADLINE).unwrap();
    stream.set_read_timeout(Some(SETUP_DEADLINE)).unwrap();
    stream.set_write_timeout(Some(SETUP_DEADLINE)).unwrap();
    stream.write_all(&[1]).unwrap();
    let mut release = [0];
    stream.read_exact(&mut release).unwrap();
    assert_eq!(release, [2]);
}

#[test]
#[ignore = "Owned readiness helper for native finite command fixtures"]
fn finite_ready_child() {
    child_ready(&std::env::var("SENTINEL_FIXTURE_READY").unwrap());
}

fn quote(path: &std::path::Path) -> String {
    #[cfg(unix)]
    return format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"));
    #[cfg(windows)]
    return format!("'{}'", path.to_string_lossy().replace('\'', "''"));
}
