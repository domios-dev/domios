use codeterm_update::headless::{
    self, transaction::InstallPort, Error, ErrorCode, VerifiedManifest,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Read,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
};

const STATE: &str = "/var/lib/domios-update";
const RELEASES: &str = "/opt/domios-releases";
const ACTIVE: &str = "/opt/domios";

fn failure(code: ErrorCode, message: impl ToString) -> Error {
    Error::new(code, message)
}

fn secure(path: &Path) -> Result<(), Error> {
    for ancestor in path.ancestors() {
        let metadata =
            fs::symlink_metadata(ancestor).map_err(|e| failure(ErrorCode::HelperUnavailable, e))?;
        if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 || metadata.file_type().is_symlink()
        {
            return Err(failure(
                ErrorCode::HelperUnavailable,
                format!("Helper path is not root-controlled: {}", ancestor.display()),
            ));
        }
    }
    Ok(())
}

fn slot(path: &Path) -> Result<(), Error> {
    if path.parent() != Some(Path::new(RELEASES)) {
        return Err(failure(
            ErrorCode::HelperUnavailable,
            "Bundle must be a direct child of /opt/domios-releases",
        ));
    }
    secure(path)
}

fn secure_tree(path: &Path) -> Result<(), Error> {
    secure(path)?;
    if path.is_dir() {
        for entry in fs::read_dir(path).map_err(|e| failure(ErrorCode::HelperUnavailable, e))? {
            secure_tree(
                &entry
                    .map_err(|e| failure(ErrorCode::HelperUnavailable, e))?
                    .path(),
            )?;
        }
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), Error> {
    File::open(path)
        .and_then(|f| f.sync_all())
        .map_err(|e| failure(ErrorCode::StageFailed, e))
}

fn write_json(name: &str, value: &impl Serialize) -> Result<(), Error> {
    let mut file =
        tempfile::NamedTempFile::new_in(STATE).map_err(|e| failure(ErrorCode::StageFailed, e))?;
    serde_json::to_writer(&mut file, value).map_err(|e| failure(ErrorCode::StageFailed, e))?;
    file.as_file()
        .set_permissions(fs::Permissions::from_mode(0o644))
        .map_err(|e| failure(ErrorCode::StageFailed, e))?;
    file.as_file()
        .sync_all()
        .map_err(|e| failure(ErrorCode::StageFailed, e))?;
    file.persist(Path::new(STATE).join(name))
        .map_err(|e| failure(ErrorCode::StageFailed, e))?;
    sync_directory(Path::new(STATE))
}

fn read_json<T: serde::de::DeserializeOwned>(name: &str) -> Result<T, Error> {
    let path = Path::new(STATE).join(name);
    secure(&path)?;
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(16385).read_to_end(&mut bytes))
        .map_err(|e| failure(ErrorCode::StageFailed, e))?;
    if bytes.len() > 16384 {
        return Err(failure(
            ErrorCode::StageFailed,
            "Helper state exceeds bound",
        ));
    }
    serde_json::from_slice(&bytes).map_err(|e| failure(ErrorCode::StageFailed, e))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    previous: PathBuf,
    next: PathBuf,
    version: String,
    build: String,
}

fn atomic_link(target: &Path) -> Result<(), Error> {
    slot(target)?;
    super::staging::switch_link(Path::new(ACTIVE), target)
}

fn restart() -> Result<(), Error> {
    let status = Command::new("/usr/bin/systemctl")
        .args(["restart", "domios-server.service"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(std::process::Stdio::null())
        .status()
        .map_err(|e| failure(ErrorCode::RestartFailed, e))?;
    if !status.success() {
        return Err(failure(ErrorCode::RestartFailed, "systemd restart failed"));
    }
    Ok(())
}

fn restart_previous() -> Result<(), Error> {
    let status = Command::new("/usr/bin/systemctl")
        .args(["reset-failed", "domios-server.service"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(std::process::Stdio::null())
        .status()
        .map_err(|e| failure(ErrorCode::RollbackFailed, e))?;
    if !status.success() {
        return Err(failure(
            ErrorCode::RollbackFailed,
            "systemd could not clear the failed activation limit",
        ));
    }
    restart()
}

fn recover(runtime: &tokio::runtime::Runtime) -> Result<(), Error> {
    if !Path::new(STATE)
        .join("pending.json")
        .try_exists()
        .map_err(|e| failure(ErrorCode::RollbackFailed, e))?
    {
        return Ok(());
    }
    let pending: Pending = read_json("pending.json")?;
    slot(&pending.previous)?;
    atomic_link(&pending.previous)?;
    sync_directory(Path::new("/opt"))?;
    restart_previous()?;
    runtime.block_on(headless::wait_for_health(&pending.version, &pending.build))?;
    fs::remove_file(Path::new(STATE).join("pending.json"))
        .map_err(|e| failure(ErrorCode::RollbackFailed, e))?;
    sync_directory(Path::new(STATE))
}

fn readable_directories(root: &Path) -> Result<(), Error> {
    for entry in fs::read_dir(root).map_err(|e| failure(ErrorCode::StageFailed, e))? {
        let entry = entry.map_err(|e| failure(ErrorCode::StageFailed, e))?;
        if entry
            .file_type()
            .map_err(|e| failure(ErrorCode::StageFailed, e))?
            .is_dir()
        {
            readable_directories(&entry.path())?;
        }
    }
    fs::set_permissions(root, fs::Permissions::from_mode(0o755))
        .map_err(|e| failure(ErrorCode::StageFailed, e))?;
    sync_directory(root)
}

struct Transaction<'a> {
    runtime: &'a tokio::runtime::Runtime,
    offer: VerifiedManifest,
    scratch: tempfile::TempDir,
    stage: Option<tempfile::TempDir>,
    previous: PathBuf,
    previous_version: String,
    previous_build: String,
    next: Option<PathBuf>,
    build: String,
}

impl Transaction<'_> {
    fn receipt(&self) -> serde_json::Value {
        serde_json::json!({"phase":"success", "status":"installed", "version":self.offer.manifest().version,
            "bundle":self.next, "previous_bundle":self.previous})
    }
}

impl InstallPort for Transaction<'_> {
    fn download(&mut self) -> Result<(), Error> {
        self.runtime.block_on(headless::download(
            &self.offer,
            &self.scratch.path().join("archive.tar.gz"),
        ))
    }
    fn verify(&mut self) -> Result<(), Error> {
        self.offer.verify_archive(headless::archive_file(
            &self.scratch.path().join("archive.tar.gz"),
        )?)
    }
    fn stage(&mut self) -> Result<(), Error> {
        let stage = self
            .stage
            .as_ref()
            .ok_or_else(|| failure(ErrorCode::StageFailed, "Staging directory missing"))?;
        self.build = super::staging::stage(
            &self.offer,
            &self.scratch.path().join("archive.tar.gz"),
            stage.path(),
        )?;
        readable_directories(stage.path())?;
        fs::set_permissions(
            stage.path().join(".domios-version"),
            fs::Permissions::from_mode(0o644),
        )
        .map_err(|e| failure(ErrorCode::StageFailed, e))?;
        Ok(())
    }
    fn swap(&mut self) -> Result<(), Error> {
        let stage = self
            .stage
            .take()
            .ok_or_else(|| failure(ErrorCode::StageFailed, "Staging directory missing"))?;
        let next = stage.path().to_path_buf();
        slot(&next)?;
        let pending = Pending {
            previous: self.previous.clone(),
            next: next.clone(),
            version: self.previous_version.clone(),
            build: self.previous_build.clone(),
        };
        write_json("pending.json", &pending)?;
        let _ = stage.keep();
        self.next = Some(next.clone());
        atomic_link(&next)
    }
    fn restart(&mut self) -> Result<(), Error> {
        sync_directory(Path::new("/opt"))?;
        restart()
    }
    fn health(&mut self) -> Result<(), Error> {
        self.runtime.block_on(headless::wait_for_health(
            &self.offer.manifest().version,
            &self.build,
        ))
    }
    fn rollback(&mut self) -> Result<(), Error> {
        atomic_link(&self.previous)?;
        sync_directory(Path::new("/opt"))
    }
    fn restart_previous(&mut self) -> Result<(), Error> {
        restart_previous()
    }
    fn health_previous(&mut self) -> Result<(), Error> {
        self.runtime.block_on(headless::wait_for_health(
            &self.previous_version,
            &self.previous_build,
        ))?;
        fs::remove_file(Path::new(STATE).join("pending.json"))
            .map_err(|e| failure(ErrorCode::RollbackFailed, e))?;
        sync_directory(Path::new(STATE))
    }
    fn commit(&mut self) -> Result<(), Error> {
        write_json("result.json", &self.receipt())?;
        fs::remove_file(Path::new(STATE).join("pending.json"))
            .map_err(|e| failure(ErrorCode::StageFailed, e))?;
        sync_directory(Path::new(STATE))
    }
}

fn execute(runtime: &tokio::runtime::Runtime) -> Result<serde_json::Value, Error> {
    recover(runtime)?;
    let previous = Path::new(ACTIVE)
        .canonicalize()
        .map_err(|e| failure(ErrorCode::HelperUnavailable, e))?;
    slot(&previous)?;
    secure_tree(&previous)?;
    let previous_version = fs::read_to_string(previous.join(".domios-version"))
        .map_err(|e| failure(ErrorCode::HelperUnavailable, e))?
        .trim()
        .to_string();
    let previous_build = super::staging::validate_bundle(&previous)?;
    let offer = runtime.block_on(headless::fetch())?;
    if offer.target_version(&previous_version)?.is_none() {
        return Ok(serde_json::json!({"phase":"idle", "status":"up_to_date"}));
    }
    let scratch = tempfile::Builder::new()
        .prefix(".download-")
        .tempdir_in(STATE)
        .map_err(|e| failure(ErrorCode::StageFailed, e))?;
    let stage = tempfile::Builder::new()
        .prefix("bundle-")
        .tempdir_in(RELEASES)
        .map_err(|e| failure(ErrorCode::StageFailed, e))?;
    let mut transaction = Transaction {
        runtime,
        offer,
        scratch,
        stage: Some(stage),
        previous,
        previous_version,
        previous_build,
        next: None,
        build: String::new(),
    };
    headless::transaction::install(&mut transaction)?;
    Ok(transaction.receipt())
}

pub(super) fn run(recovery: bool) -> Result<(), Error> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(failure(
            ErrorCode::HelperUnavailable,
            "Update helper requires root",
        ));
    }
    secure(&std::env::current_exe().map_err(|e| failure(ErrorCode::HelperUnavailable, e))?)?;
    secure(Path::new(STATE))?;
    secure(Path::new(RELEASES))?;
    secure(Path::new("/opt"))?;
    let active =
        fs::symlink_metadata(ACTIVE).map_err(|e| failure(ErrorCode::HelperUnavailable, e))?;
    if !active.file_type().is_symlink() || active.uid() != 0 {
        return Err(failure(
            ErrorCode::HelperUnavailable,
            "/opt/domios must be a root-owned release symlink",
        ));
    }
    let lock = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(Path::new(STATE).join("lock"))
        .map_err(|e| failure(ErrorCode::HelperUnavailable, e))?;
    use std::os::fd::AsRawFd;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(failure(
            ErrorCode::HelperUnavailable,
            "Another root update helper owns the install lock",
        ));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| failure(ErrorCode::HelperUnavailable, e))?;
    if recovery {
        let was_pending = Path::new(STATE).join("pending.json").exists();
        let result = recover(&runtime);
        if let Err(error) = &result {
            let error = failure(ErrorCode::RollbackFailed, error);
            write_json(
                "result.json",
                &serde_json::json!({"phase":"error", "last_error":error.message, "error":error}),
            )?;
        } else if std::env::var("SERVICE_RESULT").is_ok_and(|value| value != "success") {
            let mut response: serde_json::Value =
                read_json("result.json").unwrap_or_else(|_| serde_json::json!({}));
            if response["phase"] != "error" {
                let error = failure(
                    ErrorCode::HelperInterrupted,
                    "systemd interrupted the update helper",
                );
                response =
                    serde_json::json!({"phase":"error", "last_error":error.message, "error":error});
            }
            response["recovery_completed"] = was_pending.into();
            write_json("result.json", &response)?;
        }
        return result;
    }
    write_json(
        "result.json",
        &serde_json::json!({"phase":"applying", "status":"verifying"}),
    )?;
    let result = execute(&runtime);
    if result
        .as_ref()
        .is_ok_and(|value| value["status"] == "installed")
    {
        return Ok(());
    }
    let response = match &result {
        Ok(value) => value.clone(),
        Err(error) => {
            serde_json::json!({"phase":"error", "last_error":error.message, "error":error})
        }
    };
    write_json("result.json", &response)?;
    result.map(|_| ())
}
