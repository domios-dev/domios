pub use codeterm_service_core::*;
mod neighbors_port;
pub mod prelude;
pub mod service;
pub use neighbors_port::register_neighbors;
pub use prelude::*;

#[cfg(test)]
mod boundary_tests;
#[cfg(any(test, feature = "test-support"))]
pub mod memory_matrix;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use service::neighbors::NeighborsExt;
