use super::*;
use crate::role_preset;
use codeterm_types::agent::AgentRole;

#[test]
fn every_bundled_suffix_declares_launch_delivery() {
    for descriptor in builtins::all() {
        if role_preset::builtin_guidance(descriptor.id()).is_none() {
            continue;
        }
        let role = if descriptor.manages() {
            AgentRole::Manager
        } else {
            AgentRole::Worker
        };
        let mut preset = role_preset::resolve(Some(role), Some(descriptor.id()), None).unwrap();
        for fragment in &mut preset.fragments {
            fragment.text = "opaque-contract-fragment".into();
        }
        let placement = placement(Occasion::LaunchAssignment, Some(&preset)).unwrap_or_else(|| {
            panic!(
                "{} has a suffix but no LaunchAssignment row",
                descriptor.id()
            )
        });
        if !matches!(placement.slot, Slot::Silent) {
            let delivered = compose(
                Occasion::LaunchAssignment,
                Some(&preset),
                Some("opaque-task-body"),
            )
            .unwrap();
            assert!(
                delivered.contains("opaque-contract-fragment"),
                "{} omitted its contract",
                descriptor.id()
            );
            assert!(delivered.contains("opaque-task-body"));
        }
    }
}
