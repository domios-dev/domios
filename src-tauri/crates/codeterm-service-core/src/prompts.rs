pub use codeterm_prompts::prompts::*;

pub const AGENT_ROUTING_CONTRACT: &str = include_str!("../../../prompts/agent_routing_contract.md");

#[cfg(test)]
#[path = "prompts/tests/plan_task_flags.rs"]
mod plan_task_flags;
#[cfg(test)]
#[path = "prompts/tests/prompt_diet.rs"]
mod prompt_diet;
