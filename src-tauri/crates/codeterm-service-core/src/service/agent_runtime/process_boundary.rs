use super::model::RuntimeFailure;
use codeterm_plugin::agent_runtime::RuntimeBackgroundProcess;
use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as native;
#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as native;

pub(super) struct Member {
    pub pid: u32,
    argv: Vec<String>,
    cwd: Option<PathBuf>,
    native: native::Member,
}

pub(super) struct Boundary {
    pid: u32,
    infrastructure: HashMap<u32, native::Member>,
    #[cfg(windows)]
    job: std::sync::Arc<crate::service::windows_job::Job>,
}

fn failure(reason: impl Into<String>) -> RuntimeFailure {
    RuntimeFailure::Refused(format!("Owned runtime process cleanup: {}", reason.into()))
}

impl Boundary {
    pub(super) fn new(
        pid: u32,
        #[cfg(windows)] job: std::sync::Arc<crate::service::windows_job::Job>,
    ) -> Result<Self, RuntimeFailure> {
        if pid <= 1 {
            return Err(failure("invalid runtime process owner"));
        }
        let mut boundary = Self {
            pid,
            infrastructure: HashMap::new(),
            #[cfg(windows)]
            job,
        };
        boundary.infrastructure = boundary
            .snapshot()?
            .into_iter()
            .map(|member| (member.pid, member.native))
            .collect();
        Ok(boundary)
    }

    pub(super) fn snapshot(&self) -> Result<Vec<Member>, RuntimeFailure> {
        let ids = native::ids(self).map_err(failure)?;
        if ids.len() > 256 {
            return Err(failure("process snapshot exceeds 256 members"));
        }
        let mut members = Vec::new();
        for pid in ids {
            if let Some(member) = native::Member::open(self, pid).map_err(failure)? {
                members.push(Member {
                    pid,
                    argv: Vec::new(),
                    cwd: None,
                    native: member,
                });
            }
        }
        Ok(members)
    }

    pub(super) fn observe(&self) -> Result<Vec<Member>, RuntimeFailure> {
        let mut members = self.snapshot()?;
        let pids: Vec<_> = members
            .iter()
            .map(|member| sysinfo::Pid::from_u32(member.pid))
            .collect();
        let mut system = sysinfo::System::new();
        system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::Some(&pids),
            true,
            sysinfo::ProcessRefreshKind::nothing()
                .without_tasks()
                .with_cmd(sysinfo::UpdateKind::Always)
                .with_cwd(sysinfo::UpdateKind::Always),
        );
        for member in &mut members {
            if let Some(process) = system.process(sysinfo::Pid::from_u32(member.pid)) {
                member.argv = process
                    .cmd()
                    .iter()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect();
                member.cwd = process.cwd().map(std::path::Path::to_path_buf);
            }
            member.native.validate_identity(self).map_err(failure)?;
        }
        Ok(members)
    }

    fn infrastructure(&self, member: &Member) -> bool {
        member.pid == self.pid
            || self
                .infrastructure
                .get(&member.pid)
                .is_some_and(|original| {
                    original.token() == member.native.token() && !original.exited().unwrap_or(true)
                })
    }

    pub(super) fn verify_interrupt(
        &self,
        mut captured: Vec<Member>,
        roster: &[RuntimeBackgroundProcess],
    ) -> Result<(), RuntimeFailure> {
        captured.extend(self.observe()?);
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut unknown = Vec::new();
        for member in captured {
            if Instant::now() >= deadline {
                return Err(failure("command process verification deadline"));
            }
            if self.infrastructure(&member) || member.native.exited().map_err(failure)? {
                continue;
            }
            if !roster
                .iter()
                .any(|process| matches_command(&member, process))
            {
                unknown.push(member.pid);
                continue;
            }
            member.native.validate_identity(self).map_err(failure)?;
            member.native.terminate(self).map_err(failure)?;
            member.native.wait(deadline).map_err(failure)?;
        }
        let leftovers = self
            .snapshot()?
            .into_iter()
            .filter(|member| !self.infrastructure(member))
            .filter_map(|member| match member.native.exited() {
                Ok(true) => None,
                Ok(false) => Some(Ok(member.pid)),
                Err(error) => Some(Err(failure(error))),
            })
            .collect::<Result<Vec<_>, _>>()?;
        if !leftovers.is_empty() {
            return Err(failure(format!("unattributed live processes {leftovers:?}; server retained (observed unknown {unknown:?})")));
        }
        Ok(())
    }

    pub(super) fn force_terminate(&self) -> Result<(), RuntimeFailure> {
        native::terminate(self).map_err(failure)
    }

    pub(super) fn retire(&self) -> Result<(), RuntimeFailure> {
        let deadline = Instant::now() + Duration::from_secs(5);
        for _ in 0..4 {
            if Instant::now() >= deadline {
                return Err(failure("process tree retirement deadline"));
            }
            let members = self.snapshot()?;
            if members
                .iter()
                .all(|member| member.native.exited().unwrap_or(false))
            {
                return Ok(());
            }
            native::terminate(self).map_err(failure)?;
            for member in members {
                member.native.wait(deadline).map_err(failure)?;
            }
        }
        Err(failure(
            "process tree continued growing after observed exit events",
        ))
    }
}

fn executable(value: &str) -> String {
    let name = value.rsplit(['/', '\\']).next().unwrap_or(value);
    #[cfg(windows)]
    {
        name.trim_end_matches(".exe").to_lowercase()
    }
    #[cfg(not(windows))]
    {
        name.to_string()
    }
}

fn matches_command(member: &Member, process: &RuntimeBackgroundProcess) -> bool {
    let Some(cwd) = member.cwd.as_ref() else {
        return false;
    };
    let Ok(expected) = shell_words::split(&process.command) else {
        return false;
    };
    if expected.is_empty() || member.argv.is_empty() {
        return false;
    }
    command_matches(
        &member.argv,
        cwd,
        &expected,
        std::path::Path::new(&process.cwd),
        &process.command,
    )
}

fn normalized_path(path: &std::path::Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    path.strip_prefix("//?/")
        .unwrap_or(&path)
        .trim_end_matches('/')
        .to_string()
}

fn command_matches(
    argv: &[String],
    cwd: &std::path::Path,
    expected: &[String],
    expected_cwd: &std::path::Path,
    raw: &str,
) -> bool {
    if normalized_path(cwd) != normalized_path(expected_cwd)
        || argv.is_empty()
        || expected.is_empty()
    {
        return false;
    }
    let program = if expected[0].contains(['/', '\\']) {
        expected[0] == argv[0]
    } else {
        executable(&expected[0]) == executable(&argv[0])
    };
    let direct = program && expected[1..] == argv[1..];
    let shell_name = executable(&argv[0]);
    let switch = argv
        .get(argv.len().saturating_sub(2))
        .map(|value| value.to_ascii_lowercase());
    let command_switch = match shell_name.as_str() {
        "sh" | "bash" | "zsh" | "dash" => {
            argv.len() == 3
                && switch
                    .as_deref()
                    .is_some_and(|value| ["-c", "-lc", "-cl"].contains(&value))
        }
        "pwsh" | "powershell" => switch
            .as_deref()
            .is_some_and(|value| ["-command", "-c"].contains(&value)),
        "cmd" => switch.as_deref() == Some("/c"),
        _ => false,
    };
    let shell = command_switch && argv.last().map(String::as_str) == Some(raw);
    direct || shell
}

#[cfg(test)]
mod tests;
