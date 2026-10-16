#[path = "support/codeterm_bin.rs"]
mod codeterm_bin;

use interprocess::local_socket::{prelude::*, GenericNamespaced, ListenerOptions};
use std::io::{BufRead, BufReader, Write};
use std::time::Instant;

#[test]
fn built_binary_version_and_help_do_not_connect() {
    let name = format!("thin-help-{}", uuid::Uuid::new_v4());
    let listener = ListenerOptions::new()
        .name(name.as_str().to_ns_name::<GenericNamespaced>().unwrap())
        .create_sync()
        .unwrap();
    let server = std::thread::spawn(move || {
        let mut accepted = 0;
        loop {
            let mut stream = BufReader::new(listener.accept().unwrap());
            let mut line = String::new();
            if stream.read_line(&mut line).unwrap() == 0 {
                accepted += 1;
                continue;
            }
            if line == "stop\n" {
                return accepted;
            }
            accepted += 1;
            let request: serde_json::Value = serde_json::from_str(&line).unwrap();
            writeln!(
                stream.get_mut(),
                "{}",
                serde_json::json!({"jsonrpc":"2.0","id":request["id"],"result":{"binding":null}})
            )
            .unwrap();
        }
    });
    let binary = codeterm_bin::codeterm_bin();
    for args in [
        vec!["--version"],
        vec!["agent", "report", "--help"],
        vec!["plan", "record", "--help"],
    ] {
        let start = Instant::now();
        let output = std::process::Command::new(&binary)
            .args(&args)
            .env("CODETERM_SOCKET_PATH", &name)
            .env("CODETERM_TAB_ID", "tab_help")
            .env_remove("CODETERM_PLAN_ID")
            .env_remove("CODETERM_TASK_ID")
            .env_remove("CODETERM_INSTANCE")
            .env_remove("CODETERM_API_PORT")
            .output()
            .unwrap();
        println!("{args:?}: {:?}", start.elapsed());
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let mut stop = interprocess::local_socket::Stream::connect(
        name.as_str().to_ns_name::<GenericNamespaced>().unwrap(),
    )
    .unwrap();
    stop.write_all(b"stop\n").unwrap();
    assert_eq!(server.join().unwrap(), 0);
}
