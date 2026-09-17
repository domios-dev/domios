//! Delivery of the task-sequencing contract, asserted by constant identity.

use crate::prompts::{orchestrator_suffix, TASK_SEQUENCING_CONTRACT};

#[test]
fn plan_reminder_dependency_wait_contract_reaches_the_orchestrator_once() {
    let contract = TASK_SEQUENCING_CONTRACT.trim();
    assert!(!contract.is_empty());
    let rendered = orchestrator_suffix();
    assert_eq!(
        rendered.matches(contract).count(),
        1,
        "the sequencing contract must be delivered exactly once"
    );
    assert!(!rendered.contains("{task_sequencing_contract}"));
}
