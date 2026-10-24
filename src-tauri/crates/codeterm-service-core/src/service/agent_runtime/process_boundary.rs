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

#[cfg(test)]
impl Member {
    pub(super) fn exited(&self) -> Result<bool, RuntimeFailure> {
        self.native.exited().map_err(failure)
    }
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
                    .map(|arg| arg.to_str().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()
                    .unwrap_or_default();
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
        baseline: &[Member],
        roster: &[RuntimeBackgroundProcess],
    ) -> Result<Vec<u32>, RuntimeFailure> {
        let infrastructure = |member: &Member| {
            self.infrastructure(member)
                || baseline.iter().any(|original| {
                    original.pid == member.pid
                        && original.native.token() == member.native.token()
                        && !original.native.exited().unwrap_or(true)
                })
        };
        captured.extend(self.observe()?);
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut unknown = std::collections::BTreeSet::new();
        for member in captured {
            if infrastructure(&member) || member.native.exited().map_err(failure)? {
                continue;
            }
            if !roster
                .iter()
                .any(|process| matches_command(&member, process))
            {
                log::debug!(
                    "[agent-runtime] unattributed member retained pid={} argv={:?} cwd={:?}",
                    member.pid,
                    member.argv,
                    member.cwd
                );
                unknown.insert(member.pid);
                continue;
            }
            if Instant::now() >= deadline {
                return Err(failure(
                    "matched command verification deadline; server retained",
                ));
            }
            member.native.validate_identity(self).map_err(failure)?;
            member.native.terminate(self).map_err(failure)?;
            member.native.wait(deadline).map_err(failure)?;
        }
        for member in self.observe()? {
            if infrastructure(&member) || member.native.exited().map_err(failure)? {
                continue;
            }
            if roster
                .iter()
                .any(|process| matches_command(&member, process))
            {
                return Err(failure(format!(
                    "roster-matched process {} survived; server retained",
                    member.pid
                )));
            }
            log::debug!(
                "[agent-runtime] unattributed member retained pid={} argv={:?} cwd={:?}",
                member.pid,
                member.argv,
                member.cwd
            );
            unknown.insert(member.pid);
        }
        Ok(unknown.into_iter().collect())
    }

    pub(super) fn force_terminate(&self) -> Result<(), RuntimeFailure> {
        native::terminate(self).map_err(failure)
    }

    pub(super) fn retire(&self) -> Result<(), RuntimeFailure> {
        let deadline = Instant::now() + Duration::from_secs(5);
        #[cfg(windows)]
        let mut members = self.snapshot()?;
        #[cfg(windows)]
        let descendants = native::descendants(self).map_err(failure)?;
        let mut termination_requested = false;
        for _ in 0..4 {
            if Instant::now() >= deadline {
                return Err(failure("process tree retirement deadline"));
            }
            #[cfg(not(windows))]
            let members = self.snapshot()?;
            #[cfg(windows)]
            for pid in native::ids(self).map_err(failure)? {
                if members.iter().any(|member| member.pid == pid) {
                    continue;
                }
                if let Some(native) = native::Member::open(self, pid).map_err(failure)? {
                    members.push(Member {
                        pid,
                        argv: Vec::new(),
                        cwd: None,
                        native,
                    });
                }
            }
            let all_exited = members
                .iter()
                .map(|member| member.native.exited().map_err(failure))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .all(|exited| exited);
            if retirement_confirmed(termination_requested, all_exited) {
                #[cfg(windows)]
                for member in &descendants {
                    member.wait(deadline).map_err(failure)?;
                }
                return Ok(());
            }
            native::terminate(self).map_err(failure)?;
            #[cfg(windows)]
            for member in &descendants {
                member.terminate_retained().map_err(failure)?;
            }
            termination_requested = true;
            for member in &members {
                member.native.wait(deadline).map_err(failure)?;
            }
        }
        Err(failure(
            "process tree continued growing after observed exit events",
        ))
    }
}

fn retirement_confirmed(termination_requested: bool, all_observed_exited: bool) -> bool {
    termination_requested && all_observed_exited
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
    let Some(path) = path.to_str() else {
        return String::new();
    };
    let path = path.replace('\\', "/");
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
    if cwd.to_str().is_none()
        || expected_cwd.to_str().is_none()
        || normalized_path(cwd) != normalized_path(expected_cwd)
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
