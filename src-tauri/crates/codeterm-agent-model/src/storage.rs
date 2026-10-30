//! Shared typed Storage for operational state; Memory, plans.db and provider stores never route here.

pub use codeterm_storage::*;

#[cfg(test)]
mod tests;
