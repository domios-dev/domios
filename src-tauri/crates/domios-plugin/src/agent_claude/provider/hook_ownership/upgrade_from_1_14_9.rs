pub(super) fn command_is_owned(command: &str) -> bool {
    const MARKER: &str = "X-CodeTerm-Hook: codeterm-hook-v3"; // rename-bridge
    let Some(index) = command.find(MARKER) else {
        return false;
    };
    let is_token_char =
        |character: char| character.is_ascii_alphanumeric() || matches!(character, '_' | '-');
    let left = command[..index]
        .chars()
        .next_back()
        .is_some_and(is_token_char);
    let right = command[index + MARKER.len()..]
        .chars()
        .next()
        .is_some_and(is_token_char);
    !left && !right
}
