//! App integration tests — cross-domain tests that must not live in domain crates.

#[path = "bridge_migrate_test.rs"]
mod bridge_migrate_test;

#[path = "chatpipeline_seams_test.rs"]
mod chatpipeline_seams_test;
