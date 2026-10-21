use super::types::Task;

pub fn validate_executor_name(value: &str) -> Result<String, String> {
    let name = value.trim();
    if name.is_empty() || !name.chars().all(char::is_alphabetic) {
        return Err(format!(
            "executor name must be one word made only of letters: '{value}'"
        ));
    }
    Ok(name.to_string())
}

pub fn task_executor_name(task: &Task) -> Result<Option<String>, String> {
    task.personality
        .as_deref()
        .map(validate_executor_name)
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::validate_executor_name;

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
}
