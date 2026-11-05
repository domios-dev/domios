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

pub fn require_executor_name(value: Option<&str>) -> Result<String, String> {
    let value = value
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| {
            "personality is required: pass --personality <OneWord> (for example --personality Ada)"
                .to_string()
        })?;
    validate_executor_name(value)
}

pub fn task_executor_name(task: &Task) -> Result<Option<String>, String> {
    task.personality
        .as_deref()
        .map(validate_executor_name)
        .transpose()
}

#[cfg(test)]
mod tests;
