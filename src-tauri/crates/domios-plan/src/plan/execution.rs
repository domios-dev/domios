pub use codeterm_types::plan::execution::*;

#[cfg(test)]
mod tests {
    use super::TaskExecutionIntent;

    #[test]
    fn facade_defaults_to_repository_mutation() {
        assert!(TaskExecutionIntent::default().requires_repository());
    }
}
