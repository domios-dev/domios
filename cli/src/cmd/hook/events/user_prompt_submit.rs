//! `user_prompt_submit` hook stub. Phase 2 returns empty; Phase 5 will
//! emit tool-history-aware tips (e.g. "prefix tests with `codeterm snip`
//! for ~99% output reduction" when recent Bash calls didn't go through
//! snip, or "try `codeterm map` after 3+ Read calls in 5 turns").

use crate::args::hook::HookProvider;

pub fn handle(_provider: HookProvider, _input: &str) -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_empty_for_claude() {
        assert_eq!(handle(HookProvider::Claude, "{}"), "");
    }

    #[test]
    fn returns_empty_for_opencode() {
        assert_eq!(handle(HookProvider::Opencode, "{}"), "");
    }

    #[test]
    fn returns_empty_on_garbage_input() {
        assert_eq!(handle(HookProvider::Claude, "not json"), "");
    }
}
