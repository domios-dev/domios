use super::*;
use crate::config::MemoryConfig;

#[test]
fn memory_backend_defaults_to_sqlite() {
    let config: MemoryConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(config.backend, MemoryBackend::Sqlite);
}

#[test]
fn memory_backend_selection_is_strict() {
    for backend in ["sqlite", "postgres"] {
        let config: MemoryConfig =
            serde_json::from_value(serde_json::json!({"backend": backend})).unwrap();
        assert_eq!(config.backend.to_string(), backend);
    }
    for backend in ["", "postgress", "SQLite"] {
        assert!(
            serde_json::from_value::<MemoryConfig>(serde_json::json!({"backend": backend}))
                .is_err()
        );
    }
}
