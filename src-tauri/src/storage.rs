//! Shared typed Storage for operational state. Dormant: no startup path opens it until the
//! reviewed all-domain cutover; Memory, plans.db and provider stores never route here.

pub use codeterm_storage::*;

pub mod verification;

#[allow(dead_code)] // The all-domain cutover calls the importer; tests drive it until then.
mod migration;
#[cfg(test)]
mod migration_tests;
#[cfg(test)]
mod tests;
