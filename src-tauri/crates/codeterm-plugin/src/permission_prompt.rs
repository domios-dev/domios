/// A provider-owned answer to its own tool-permission prompt: the affirmative
/// option, the rule that recognized it, and the key steps that select it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionPromptResponse {
    pub option_index: usize,
    pub rule: String,
    pub steps: Vec<Vec<u8>>,
}
