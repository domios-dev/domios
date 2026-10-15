use super::*;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

struct ParkedWriter {
    entered: Option<Sender<()>>,
    release: Receiver<()>,
    written: Sender<Vec<u8>>,
}

impl Write for ParkedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if let Some(entered) = self.entered.take() {
            entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(3)).unwrap();
        }
        self.written
            .send(bytes.to_vec())
            .map_err(std::io::Error::other)?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct Capture(Sender<Vec<u8>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.send(bytes.to_vec()).map_err(std::io::Error::other)?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn parked_writer_never_blocks_callers_and_full_queue_counts_drops() {
    let (entered_tx, entered_rx) = channel();
    let (release_tx, release_rx) = channel();
    let (written_tx, written_rx) = channel();
    let sink = NonBlockingLog::spawn(
        "parked",
        ParkedWriter {
            entered: Some(entered_tx),
            release: release_rx,
            written: written_tx,
        },
        1,
        Vec::new(),
    )
    .unwrap();
    sink.submit(Arc::from(&b"first\n"[..]));
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    sink.submit(Arc::from(&b"queued\n"[..]));
    let drops = sink.drops();
    let (completed_tx, completed_rx) = channel();
    let caller = std::thread::spawn(move || {
        for _ in 0..1000 {
            sink.submit(Arc::from(&b"overflow\n"[..]));
        }
        completed_tx.send(()).unwrap();
    });
    let completed = completed_rx.recv_timeout(Duration::from_secs(1));
    release_tx.send(()).unwrap();
    caller.join().unwrap();
    completed.unwrap();
    assert_eq!(drops.total(), 1000);
    assert_eq!(
        written_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        b"first\n"
    );
    let mut output = Vec::new();
    loop {
        match written_rx.recv_timeout(Duration::from_secs(1)) {
            Ok(bytes) => output.extend(bytes),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(error) => panic!("writer did not finish: {error}"),
        }
    }
    assert!(String::from_utf8(output)
        .unwrap()
        .contains("[log] dropped 1000 records sink=parked"));
}

#[test]
fn slow_stderr_drops_are_reported_to_independent_file_sink() {
    let (entered_tx, entered_rx) = channel();
    let (release_tx, release_rx) = channel();
    let (written_tx, _written_rx) = channel();
    let stderr = NonBlockingLog::spawn(
        "stderr",
        ParkedWriter {
            entered: Some(entered_tx),
            release: release_rx,
            written: written_tx,
        },
        1,
        Vec::new(),
    )
    .unwrap();
    stderr.submit(Arc::from(&b"first\n"[..]));
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    stderr.submit(Arc::from(&b"queued\n"[..]));
    stderr.submit(Arc::from(&b"dropped\n"[..]));
    let (file_tx, file_rx) = channel();
    let file = NonBlockingLog::spawn(
        "file",
        Capture(file_tx),
        4,
        vec![("stderr", stderr.drops())],
    )
    .unwrap();
    file.submit(Arc::from(&b"durable\n"[..]));
    let record = file_rx.recv_timeout(Duration::from_secs(1));
    let report = file_rx.recv_timeout(Duration::from_secs(1));
    release_tx.send(()).unwrap();
    assert_eq!(record.unwrap(), b"durable\n");
    assert_eq!(
        String::from_utf8(report.unwrap()).unwrap(),
        "[log] dropped 1 records sink=stderr\n"
    );
    assert_eq!(file.drops().total(), 0);
}

#[test]
fn oversized_and_disconnected_records_are_counted_without_io() {
    let (sender, receiver) = sync_channel(1);
    let sink = NonBlockingLog {
        sender,
        drops: LogDropCounter::default(),
    };
    sink.submit(vec![0; MAX_RECORD_BYTES + 1].into());
    assert_eq!(sink.drops().total(), 1);
    drop(receiver);
    sink.submit(Arc::from(&b"closed"[..]));
    assert_eq!(sink.drops().total(), 2);
}

#[test]
fn drop_reports_are_independent_per_sink_and_retry_after_write_failure() {
    let drops = LogDropCounter::default();
    drops.record();
    let mut failed = 0;
    drops.report(
        &mut std::io::Cursor::new(&mut [0u8; 0][..]),
        "stderr",
        &mut failed,
    );
    assert_eq!(failed, 0);
    let mut first = Vec::new();
    drops.report(&mut first, "stderr", &mut failed);
    assert_eq!(failed, 1);
    let mut second = Vec::new();
    let mut independently_reported = 0;
    drops.report(&mut second, "stderr", &mut independently_reported);
    assert_eq!(first, second);
    let length = first.len();
    drops.report(&mut first, "stderr", &mut failed);
    assert_eq!(first.len(), length);
}
