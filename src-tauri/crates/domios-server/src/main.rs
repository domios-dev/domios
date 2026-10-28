use domios_server::{
    bootstrap::{
        channel,
        protocol::{with_observation_preflight, AttachReply, AttachRequest, FAILURE_DEADLINE},
    },
    launch::{
        human_start_options, launch_service, run_isolated_verification, write_human_ready,
        HumanStartMode, LaunchOptions, HUMAN_START_USAGE,
    },
};
use std::io::{self, BufRead, Read, Write};

fn main() {
    if let Err(error) = execute() {
        eprintln!("domios-server refused: {error}");
        std::process::exit(channel::helper_exit_code(&error));
    }
}

fn execute() -> io::Result<()> {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("pty-child")) {
        let exit = domios_server::pty_child::run(std::env::args_os().skip(2))?;
        std::process::exit(exit);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(args.as_slice(), [arg] if arg == "headless-update" || arg == "headless-update-recover")
    {
        return domios_server::headless_update::run_helper(args[0] == "headless-update-recover");
    }
    if matches!(args.as_slice(), [arg] if arg == "--help" || arg == "help")
        || matches!(args.as_slice(), [command, arg] if command == "start" && arg == "--help")
    {
        println!("{HUMAN_START_USAGE}");
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("start") {
        let executable = std::env::current_exe()?.canonicalize()?;
        let start = human_start_options(&args[1..], &executable, cfg!(debug_assertions))?;
        if start.mode == HumanStartMode::IsolatedVerification {
            return run_isolated_verification(&executable, &start.options);
        }
        let pid_file = start
            .pid_file
            .as_deref()
            .map(domios_server::launch::pid_file::PidFile::prepare)
            .transpose()?;
        let reply = launch_service(&executable, &start.options)?;
        if let Some(pid_file) = pid_file {
            pid_file.publish(reply.ready.process.pid)?;
        }
        return write_human_ready(&mut io::stdout().lock(), &reply.ready, start.json);
    }
    if !private_stdio()? {
        return Err(io::Error::other(
            "Bootstrap requires inherited private input/output pipes",
        ));
    }
    let command = std::env::args().nth(1).ok_or_else(|| {
        io::Error::other(
            "Expected launch, serve, attach, or observe; options arrive on protected stdin",
        )
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
            let reply = launch_service(&std::env::current_exe()?, &options)?;
            let result = write_reply(&reply);
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
