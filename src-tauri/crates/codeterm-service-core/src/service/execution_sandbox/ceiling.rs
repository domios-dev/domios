use codeterm_types::plan::{ExecutionIntent, ExecutionSandboxPolicy};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Access {
    ReadOnly,
    WorkspaceWrite,
    Full,
}

impl Access {
    fn confined(intent: Option<ExecutionIntent>) -> Self {
        match intent {
            None => Self::Full,
            Some(ExecutionIntent::ReadOnly) => Self::ReadOnly,
            Some(_) => Self::WorkspaceWrite,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
            Self::Full => "full access",
        }
    }
}

/// Refuses a delegated launch whose requested access exceeds the machine
/// ceiling or any confined caller's recorded policy; omitting `--sandbox` requests full access.
pub(super) fn check(
    requested: Option<&str>,
    machine_ceiling: Option<&str>,
    callers: &[(String, ExecutionSandboxPolicy)],
    cwd: Option<&str>,
) -> Result<(), String> {
    let requested = Access::confined(super::requested_intent(requested)?);
    if let Some(ceiling) = machine_ceiling {
        let ceiling = super::requested_intent(Some(ceiling))
            .map_err(|_| "agent.sandbox_ceiling must be read-only or workspace-write")?;
        refuse_above(
            requested,
            Access::confined(ceiling),
            "the machine (agent.sandbox_ceiling)",
        )?;
    }
    for (tab, policy) in callers {
        refuse_above(
            requested,
            Access::confined(Some(policy.intent.clone())),
            &format!("caller tab {tab}"),
        )?;
        if requested == Access::WorkspaceWrite && !inside_writable_roots(cwd, policy) {
            return Err(format!(
                "Delegated workspace-write launch must stay inside the writable roots of caller tab {tab}"
            ));
        }
    }
    Ok(())
}

fn refuse_above(requested: Access, ceiling: Access, source: &str) -> Result<(), String> {
    if requested <= ceiling {
        return Ok(());
    }
    Err(format!(
        "Delegated launch requests {}, above the {} sandbox ceiling of {source}; pass --sandbox {} or narrower",
        requested.label(),
        ceiling.label(),
        ceiling.label(),
    ))
}

fn inside_writable_roots(cwd: Option<&str>, policy: &ExecutionSandboxPolicy) -> bool {
    let Some(cwd) = cwd
        .map(Path::new)
        .and_then(|path| path.canonicalize().ok())
        .and_then(|path| path.to_str().map(codeterm_paths::strip_verbatim_prefix))
    else {
        return false;
    };
    std::iter::once(&policy.cwd)
        .chain(&policy.writable_roots)
        .any(|root| Path::new(&cwd).starts_with(root))
}

#[cfg(test)]
mod tests;
