//! JSON-RPC error codes shared across the socket server.
//!
//! Application conditions use the implementation-defined server-error band
//! (-32099..-32000); protocol conditions keep their JSON-RPC 2.0 codes.

/// A well-formed request referenced an object that does not exist.
pub(super) const NOT_FOUND: i32 = -32001;

/// A plan owner verb was attempted without the live Orchestrator binding.
pub(super) const PLAN_ORCHESTRATOR_REQUIRED: i32 = -32003;

/// JSON-RPC 2.0 "Invalid params": the request itself is malformed or under-specified.
pub(super) const INVALID_PARAMS: i32 = -32602;
