/// A provider's answer to its own directory-trust dialog, read from the screen.
/// One key per decision: the next observed frame decides the next key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectoryTrustPromptResponse {
    Answer {
        step: Vec<u8>,
    },
    /// A trust dialog this provider will not answer; it stays with the user.
    Hold,
}

impl DirectoryTrustPromptResponse {
    /// Anything but exactly one non-empty key fails closed to `Hold`.
    pub fn from_wire_steps(steps: Vec<Vec<u8>>) -> Self {
        match <[Vec<u8>; 1]>::try_from(steps) {
            Ok([step]) if !step.is_empty() => Self::Answer { step },
            _ => Self::Hold,
        }
    }
}
