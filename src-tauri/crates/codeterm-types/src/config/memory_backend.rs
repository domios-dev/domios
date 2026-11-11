use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum MemoryBackend {
    #[default]
    Sqlite,
    Postgres,
}

impl MemoryBackend {
    pub fn other(self) -> Self {
        match self {
            Self::Sqlite => Self::Postgres,
            Self::Postgres => Self::Sqlite,
        }
    }
}

impl std::fmt::Display for MemoryBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgres",
        })
    }
}

#[cfg(test)]
mod tests;
