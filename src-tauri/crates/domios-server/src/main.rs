use domios_server::{
    bootstrap::{
        channel,
        protocol::{with_observation_preflight, AttachReply, AttachRequest, FAILURE_DEADLINE},
    },
    launch::{spawn_detached, LaunchOptions},
};
use std::io::{self, BufRead, Read, Write};

fn main() {
    if let Err(error) = execute() {
        eprintln!("domios-server refused: {error}");
        std::process::exit(if error.kind() == io::ErrorKind::AlreadyExists {
            3
        } else {
            1
        });
    }
}

fn execute() -> io::Result<()> {
    if !private_stdio()? {
        return Err(io::Error::other(
            "Bootstrap requires inherited private input/output pipes",
        ));
    }
    let command = std::env::args().nth(1).ok_or_else(|| {
        io::Error::other("Expected launch, serve, or attach; options arrive on protected stdin")
    })?;
    let mut input = String::new();
    std::io::stdin().lock().take(32769).read_line(&mut input)?;
    if input.len() > 32768 || !input.ends_with('\n') {
        return Err(io::Error::other("Bootstrap options exceed bound"));
    }
    if command == "attach" || command == "observe" {
        let request: AttachRequest = serde_json::from_str(&input).map_err(io::Error::other)?;
        let attach = || -> io::Result<_> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(channel::attach(&request, &std::env::current_exe()?))
        };
        let (reply, pin) = if command == "observe" {
            with_observation_preflight(&request, attach)?
        } else {
            attach()
        }?;
        if command == "observe" {
            let mut stdout = io::stdout().lock();
            writeln!(
                stdout,
                "{}",
                serde_json::json!({ "observing": reply.ready })
            )?;
            stdout.flush()?;
            pin.wait_exit(FAILURE_DEADLINE)?;
            writeln!(stdout, "{}", serde_json::json!({ "exited": true }))?;
            return stdout.flush();
        }
        return write_reply(&reply);
    }
    let options: LaunchOptions = serde_json::from_str(&input).map_err(io::Error::other)?;
    options.validate(cfg!(debug_assertions))?;
    match command.as_str() {
        "serve" => domios_server::run(options, |ready, token| {
            write_reply(&AttachReply {
                challenge: String::new(),
                ready: ready.clone(),
                api_token: token.map(str::to_string),
            })
        }),
        "launch" => {
            let mut child = spawn_detached(&std::env::current_exe()?)?;
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| io::Error::other("Child bootstrap pipe unavailable"))?;
            serde_json::to_writer(&mut stdin, &options).map_err(io::Error::other)?;
            stdin.write_all(b"\n")?;
            drop(stdin);
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| io::Error::other("Child readiness pipe unavailable"))?;
            let (sender, receiver) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let mut line = String::new();
                let result = io::BufReader::new(stdout)
                    .take(32769)
                    .read_line(&mut line)
                    .map(|_| line);
                let _ = sender.send(result);
            });
            let mut initialized = false;
            let result = (|| -> io::Result<()> {
                match receiver.recv_timeout(FAILURE_DEADLINE) {
                    Ok(Ok(line)) if !line.is_empty() && line.len() <= 32768 => {
                        let reply: AttachReply =
                            serde_json::from_str(&line).map_err(io::Error::other)?;
                        let pin =
                            domios_server::bootstrap::process::PinnedProcess::open(child.id())?;
                        let observed = pin.provenance()?;
                        if reply.ready.process != observed
                            || observed.executable != std::env::current_exe()?.canonicalize()?
                        {
                            Err(io::Error::other("Launcher/server provenance mismatch"))
                        } else {
                            initialized = true;
                            write_reply(&reply)
                        }
                    }
                    _ => Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Server launch failed before readiness",
                    )),
                }
            })();
            if result.is_err() && !initialized && child.try_wait()?.is_none() {
                child.kill()?;
                child.wait()?;
            }
            if result.is_ok() && std::env::args().nth(2).as_deref() == Some("--hold") {
                let mut discarded = [0u8; 1];
                let _ = io::stdin().read(&mut discarded)?;
            }
            result
        }
        _ => Err(io::Error::other("Unknown server bootstrap operation")),
    }
}

fn write_reply(reply: &AttachReply) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, reply).map_err(io::Error::other)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

fn private_stdio() -> io::Result<bool> {
    #[cfg(unix)]
    {
        for fd in [libc::STDIN_FILENO, libc::STDOUT_FILENO] {
            let mut metadata: libc::stat = unsafe { std::mem::zeroed() };
            if unsafe { libc::fstat(fd, &mut metadata) } < 0 {
                return Err(io::Error::last_os_error());
            }
            let kind = metadata.st_mode & libc::S_IFMT;
            if kind != libc::S_IFIFO && kind != libc::S_IFSOCK {
                return Ok(false);
            }
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Storage::FileSystem::{GetFileType, FILE_TYPE_PIPE},
            System::Console::{GetStdHandle, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE},
        };
        for kind in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE] {
            if unsafe { GetFileType(GetStdHandle(kind)) } != FILE_TYPE_PIPE {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
