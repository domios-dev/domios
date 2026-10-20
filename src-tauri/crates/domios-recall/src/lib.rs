pub mod registry;
pub mod search;
#[cfg(test)]
mod test_support;

pub use registry::{DecisionBackend, SupporterRegistry};
pub use search::{RecallBackend, SearchError, SearchParams, SearchQuery};

extern crate domios_agent_model as codeterm_agent_model; // rename-bridge
extern crate domios_config as codeterm_config; // rename-bridge
extern crate domios_decision as codeterm_decision; // rename-bridge
extern crate domios_mem as codeterm_mem; // rename-bridge
extern crate domios_paths as codeterm_paths; // rename-bridge
extern crate domios_plan as codeterm_plan; // rename-bridge
extern crate domios_supporter as codeterm_supporter; // rename-bridge
extern crate domios_types as codeterm_types; // rename-bridge
