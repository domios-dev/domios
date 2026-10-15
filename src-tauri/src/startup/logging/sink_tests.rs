use super::*;
use codeterm_util::nonblocking_log::NonBlockingLog;
use std::{
    io::Write,
    sync::{mpsc, Arc},
    time::Duration,
};

struct Parked {
    entered: Option<mpsc::Sender<()>>,
    release: mpsc::Receiver<()>,
}

impl Write for Parked {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if let Some(entered) = self.entered.take() {
            entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(3)).unwrap();
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct Capture(mpsc::Sender<Vec<u8>>);
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
fn fern_logging_completes_and_file_receives_records_while_stderr_is_parked() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let stderr = NonBlockingLog::spawn(
        "stderr",
        Parked {
            entered: Some(entered_tx),
            release: release_rx,
        },
        1,
        Vec::new(),
    )
    .unwrap();
    let drops = stderr.drops();
    let (file_tx, file_rx) = mpsc::channel();
    let file = NonBlockingLog::spawn(
        "file",
        Capture(file_tx),
        128,
        vec![("stderr", drops.clone())],
    )
    .unwrap();
    let file_drops = file.drops();
    let (_, logger) = logging_dispatch(Some(stderr), Some(file))
        .level(log::LevelFilter::Debug)
        .into_log();
    let logger = Arc::new(logger);
    logger.log(
        &log::Record::builder()
            .level(log::Level::Info)
            .target("test")
            .args(format_args!("begin"))
            .build(),
    );
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let (completed_tx, completed_rx) = mpsc::channel();
    let caller = std::thread::spawn(move || {
        for index in 0..32 {
            logger.log(
                &log::Record::builder()
                    .level(log::Level::Debug)
                    .target("test")
                    .args(format_args!("record-{index}"))
                    .build(),
            );
        }
        completed_tx.send(()).unwrap();
    });
    let completed = completed_rx.recv_timeout(Duration::from_secs(1));
    let mut records = String::new();
    let received = loop {
        match file_rx.recv_timeout(Duration::from_secs(1)) {
            Ok(bytes) => {
                records.push_str(&String::from_utf8(bytes).unwrap());
                if records.contains("record-31\n") {
                    break true;
                }
            }
            Err(_) => break false,
        }
    };
    release_tx.send(()).unwrap();
    caller.join().unwrap();
    completed.unwrap();
    assert!(
        received,
        "file delivery must finish before stderr is released"
    );
    assert_eq!(file_drops.total(), 0);
    assert_eq!(drops.total(), 31);
    assert!(records.contains(" INFO test] begin\n"));
    for index in 0..32 {
        assert!(records.contains(&format!(" DEBUG test] record-{index}\n")));
    }
}
