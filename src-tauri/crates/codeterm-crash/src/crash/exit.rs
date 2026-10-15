//! Exit cleanup, split into two independently-runnable halves.
//!
//! Persisting state is safe on any `RunEvent::Exit`, including one we never
//! asked for; declaring a clean exit (sentinel + keepawake/webview teardown) is
//! only correct on a genuine quit.

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitPlan {
    pub declare_clean_exit: bool,
    pub persist_state: bool,
}

/// macOS delivers `RunEvent::Exit` with no preceding quit request on routes we
/// never asked for; persist there anyway, but never tear down keepawake children.
pub fn plan_run_exit(is_macos: bool, shutting_down: bool) -> ExitPlan {
    ExitPlan {
        declare_clean_exit: !is_macos || shutting_down,
        persist_state: true,
    }
}

pub fn execute_exit_plan<D, P>(plan: ExitPlan, declare: D, persist: P)
where
    D: FnOnce(),
    P: FnOnce(),
{
    if plan.declare_clean_exit {
        declare();
    }
    if plan.persist_state {
        persist();
    }
}

pub fn shutdown_state_persistence_result(errors: &[String]) -> Result<(), String> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}
