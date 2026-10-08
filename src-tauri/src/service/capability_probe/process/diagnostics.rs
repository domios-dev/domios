use super::*;
use std::{io, process::ExitStatus, thread::JoinHandle};

pub(super) struct Stderr(JoinHandle<io::Result<Vec<u8>>>);

impl Stderr {
    pub(super) fn capture(child: &mut std::process::Child) -> Self {
        let mut stderr = child.stderr.take().expect("diagnostic stderr pipe");
        Self(std::thread::spawn(move || {
            let mut prefix = Vec::new();
            stderr.by_ref().take(4096).read_to_end(&mut prefix)?;
            io::copy(&mut stderr, &mut io::sink())?;
            Ok(prefix)
        }))
    }

    pub(super) fn report(
        self,
        command: &Command,
        result: &Result<InitializeCapabilities, ProbeFailure>,
        before_cleanup: &io::Result<Option<ExitStatus>>,
        status: &io::Result<ExitStatus>,
    ) {
        let stderr = self.0.join();
        if result.is_err() {
            report_path("program", Path::new(command.get_program()));
            if let Some(script) = command.get_args().next() {
                report_path("first_argument", Path::new(script));
            }
            if let Some(cwd) = command.get_current_dir() {
                report_path("cwd", cwd);
                report_path("progress", &cwd.join("progress.jsonl"));
                report_path("receipt", &cwd.join("receipt.json"));
            }
            eprintln!(
                "probe child program={:?} args={:?} cwd={:?} before_cleanup={before_cleanup:?} status={status:?} stderr={:?}",
                command.get_program(),
                command.get_args().collect::<Vec<_>>(),
                command.get_current_dir(),
                stderr.map(|result| result.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
            );
        }
    }
}

fn report_path(label: &str, path: &Path) {
    #[cfg(windows)]
    let units = {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str().encode_wide().count()
    };
    #[cfg(not(windows))]
    let units = path.as_os_str().as_encoded_bytes().len();
    eprintln!("probe path {label} length={units} path={path:?}");
}
