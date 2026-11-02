pub use codeterm_crash::crash::exit::{
    execute_exit_plan, plan_run_exit, shutdown_state_persistence_result, ExitPlan,
};

#[cfg(test)]
mod tests;
