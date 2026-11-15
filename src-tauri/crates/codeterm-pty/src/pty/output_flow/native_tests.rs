use super::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;

struct KillOnDrop(Option<Box<dyn portable_pty::ChildKiller + Send + Sync>>);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        if let Some(mut killer) = self.0.take() {
            let _ = killer.kill();
        }
    }
}

const RECORDS: usize = 65536;
const CHILD_TEST: &str = "pty::output_flow::native_tests::output_flow_fixture_child";
const CHILD_PORT: &str = "DOMIOS_OUTPUT_FLOW_FIXTURE_PORT";
const FAILURE_BOUND: Duration = Duration::from_secs(10);

fn record(index: usize) -> String {
    format!("FLOW:{index:08}:abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMN")
}

#[test]
#[ignore = "Controlled native PTY fixture; explicitly selected by its parent"]
fn output_flow_fixture_child() {
    let Ok(address) = std::env::var(CHILD_PORT) else {
        return;
    };
    #[cfg(unix)]
    unsafe {
        let mut mode = std::mem::zeroed();
        assert_eq!(libc::tcgetattr(0, &mut mode), 0);
        mode.c_lflag &= !(libc::ECHO | libc::ICANON);
        mode.c_cc[libc::VMIN] = 1;
        assert_eq!(libc::tcsetattr(0, libc::TCSANOW, &mode), 0);
    }
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Console::{GetStdHandle, SetConsoleMode, STD_INPUT_HANDLE};
        assert_ne!(SetConsoleMode(GetStdHandle(STD_INPUT_HANDLE), 0), 0);
    }
    let mut signal = TcpStream::connect(address).unwrap();
    signal.write_all(b"R").unwrap();
    let input = std::thread::spawn(move || {
        let mut bytes = [0; 6];
        std::io::stdin().read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"FLOWIN");
        signal.write_all(b"I").unwrap();
    });
    {
        let mut output = std::io::stdout().lock();
        for index in 0..RECORDS {
            writeln!(output, "{}", record(index)).unwrap();
        }
        output.flush().unwrap();
    }
    input.join().unwrap();
}

#[test]
#[ignore = "Native ConPTY/Unix pause-input-drain qualification; requires exclusive PTY capacity"]
fn native_pause_preserves_input_and_every_output_record() {
    #[cfg(windows)]
    let _exclusive = crate::pty::windows_test_support::exclusive_conpty_test();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let pair = portable_pty::native_pty_system()
        .openpty(portable_pty::PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = portable_pty::CommandBuilder::new(std::env::current_exe().unwrap());
    command.args(["--exact", CHILD_TEST, "--ignored", "--nocapture"]);
    command.env(CHILD_PORT, address.to_string());
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let mut killer = KillOnDrop(Some(child.clone_killer()));
    let (connected_tx, connected_rx) = mpsc::channel();
    let accept = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let _ = connected_tx.send(stream);
    });
    let writer = crate::pty::write_queue::WriteQueue::new_cancellable(
        pair.master.take_cancellable_writer().unwrap(),
    );
    let mut raw = pair.master.try_clone_reader().unwrap();
    let flow = Arc::new(OutputFlow::default());
    let view = Uuid::new_v4();
    let attachment = Uuid::new_v4();
    flow.bind(1);
    assert!(flow.attach(1, view, attachment, 0, Instant::now()));
    let reader_flow = flow.clone();
    let (paused_tx, paused_rx) = mpsc::channel();
    let (drained_tx, drained_rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let mut bytes = Vec::new();
        let mut paused_tx = Some(paused_tx);
        loop {
            if reader_flow.pause_deadline(Instant::now()).is_some() {
                if let Some(sender) = paused_tx.take() {
                    let _ = sender.send(bytes.len());
                }
                runtime.block_on(reader_flow.wait_to_read());
            }
            let mut chunk = [0; 8192];
            let count = raw.read(&mut chunk).unwrap_or(0);
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..count]);
            assert!(
                bytes.len() <= 16 * 1024 * 1024,
                "native fixture output exceeded its bound"
            );
            reader_flow.produced(bytes.len() as u64);
        }
        let _ = drained_tx.send(bytes);
    });
    let (exit_tx, exit_rx) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        let _ = exit_tx.send(child.wait());
    });
    let connected = connected_rx.recv_timeout(FAILURE_BOUND);
    if connected.is_err() {
        let _ = TcpStream::connect(address);
    }
    accept.join().unwrap();
    let mut signal = connected.unwrap();
    signal.set_read_timeout(Some(FAILURE_BOUND)).unwrap();
    let mut ready = [0];
    signal.read_exact(&mut ready).unwrap();
    assert_eq!(&ready, b"R");
    let paused = paused_rx.recv_timeout(FAILURE_BOUND);
    let written = writer.send_and_wait(b"FLOWIN".to_vec(), FAILURE_BOUND);
    let mut received = [0];
    let input_while_paused = signal.read_exact(&mut received).map(|()| received == *b"I");
    let still_paused = flow.pause_deadline(Instant::now()).is_some();
    flow.detach(view, Some(attachment));
    let exit = exit_rx.recv_timeout(FAILURE_BOUND);
    if exit.as_ref().is_ok_and(|result| result.is_ok()) {
        killer.0.take();
    } else if let Some(killer) = killer.0.as_mut() {
        let _ = killer.kill();
    }
    writer.close();
    let close = std::thread::spawn(move || drop(pair.master));
    let drained = drained_rx.recv_timeout(FAILURE_BOUND);
    assert!(drained.is_ok(), "PTY did not drain after detach");
    reader.join().unwrap();
    close.join().unwrap();
    waiter.join().unwrap();
    eprintln!("paused={paused:?} write={written:?} input_while_paused={input_while_paused:?} still_paused={still_paused} drained_bytes={}", drained.as_ref().map_or(0, Vec::len));
    let paused_bytes = paused.expect("reader never reached its ACK watermark");
    assert!((HIGH_WATERMARK as usize..HIGH_WATERMARK as usize + 8192).contains(&paused_bytes));
    assert!(
        written.is_ok(),
        "PTY input write failed while output was paused: {written:?}"
    );
    assert!(
        input_while_paused.unwrap_or(false) && still_paused,
        "child could not consume input while output was paused; keep this platform exempt"
    );
    assert!(
        exit.unwrap().unwrap().success(),
        "fixture child did not exit cleanly"
    );
    let bytes = drained.unwrap();
    let clean = crate::pty::ansi::strip_ansi(&String::from_utf8(bytes).unwrap()).replace('\r', "");
    let records: Vec<_> = clean
        .split("FLOW:")
        .skip(1)
        .map(|tail| tail.chars().take(59).collect::<String>())
        .collect();
    let expected: Vec<_> = (0..RECORDS)
        .map(|index| record(index)[5..].to_string())
        .collect();
    assert_eq!(
        records, expected,
        "pause/detach lost or reordered output bytes"
    );
}
