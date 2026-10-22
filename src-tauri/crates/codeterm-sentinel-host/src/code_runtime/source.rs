use super::RuntimeFailure;

#[cfg(test)]
pub(super) fn read(directory: &std::path::Path, relative: &str) -> Result<String, RuntimeFailure> {
    codeterm_sentinel::code_source::read(directory, relative).map_err(RuntimeFailure::new)
}
