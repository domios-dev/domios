use super::{require_executor_name, validate_executor_name};

#[test]
fn tab_name_contract_accepts_trimmed_one_word_executor_names() {
    assert_eq!(validate_executor_name("  Ada ").unwrap(), "Ada");
    assert_eq!(validate_executor_name("Хоппер").unwrap(), "Хоппер");
}

#[test]
fn tab_name_contract_rejects_role_slugs_and_multi_word_names() {
    assert!(validate_executor_name("rust-backend-dev").is_err());
    assert!(validate_executor_name("Ada Lovelace").is_err());
    assert!(validate_executor_name("   ").is_err());
}

#[test]
fn personality_creation_requires_explicit_valid_name() {
    for missing in [None, Some(""), Some("   ")] {
        assert!(require_executor_name(missing)
            .unwrap_err()
            .contains("personality is required: pass --personality <OneWord>"));
    }
    for invalid in ["two words", "rust-backend-dev", "123"] {
        assert!(require_executor_name(Some(invalid)).is_err());
    }
    assert_eq!(require_executor_name(Some(" Ada ")).unwrap(), "Ada");
}
