//! Request extraction and peer access policy shared by the REST surface and the core.

pub mod access_policy;
pub(crate) mod optional_json;
pub use optional_json::OptionalJson;
