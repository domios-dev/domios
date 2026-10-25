pub(super) struct Client(std::process::Child);

impl Client {
    pub(super) fn new() -> Self {
        // cmd.exe exits at once without a console, so Windows uses a small process that waits on its own.
        #[cfg(windows)]
        let mut command = {
            let executable = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
                .join("System32/PING.EXE");
            let mut command = std::process::Command::new(executable);
            command.args(["-n", "3600", "127.0.0.1"]);
            command
        };
        #[cfg(unix)]
        let mut command = std::process::Command::new("/bin/sh");
        Self(
            command
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        )
    }

    pub(super) fn id(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.0.kill();
        self.0.wait().unwrap();
    }
}
