pub mod dream_hygiene;
pub mod embedder;
pub mod model_defaults;
pub mod schema;
pub mod supporter;
pub mod tab_sleep;

pub use dream_hygiene::DreamHygieneConfig;
pub use embedder::EmbedderConfig;
pub use model_defaults::{ModelDefaultsConfig, ReplacedModelDefault};
pub use schema::MemoryConfig;
pub use supporter::{
    MemoryRerankConfig, RerankRequestMode, SupporterConfig, SupporterConnection, SupporterSources,
    DEFAULT_SUPPORTER_CALLS_PER_HOUR,
};

pub mod utility_tabs;

mod updater;
