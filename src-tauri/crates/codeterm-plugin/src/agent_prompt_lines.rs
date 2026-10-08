//! The one-line launch-prompt pointers core plugins contribute (`agentPromptLine`),
//! composed into a single section every agent receives. Pure — the host owns
//! the registry snapshot and where the section lands.

pub const MAX_AGENT_PROMPT_LINE_CHARS: usize = 240;
pub const BUILT_IN_PLUGINS_HEADING: &str = "Built-in plugins:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginPromptLine {
    pub id: String,
    /// Verified core, or a bundled system id in a dev run; decided by the loader.
    pub core: bool,
    pub enabled: bool,
    pub line: Option<String>,
}

/// The trimmed line, or why it is refused.
pub fn validate_agent_prompt_line(raw: &str) -> Result<String, &'static str> {
    let line = raw.trim();
    if line.is_empty() {
        return Err("agentPromptLine is empty");
    }
    if line.chars().any(char::is_control) {
        return Err("agentPromptLine must be a single line");
    }
    if line.chars().count() > MAX_AGENT_PROMPT_LINE_CHARS {
        return Err("agentPromptLine exceeds 240 characters");
    }
    Ok(line.to_string())
}

/// Only enabled core plugins contribute; third-party text never reaches every
/// agent's prompt. `None` when nothing contributes.
pub fn compose_built_in_plugins_section(plugins: &[PluginPromptLine]) -> Option<String> {
    let mut lines: Vec<(&str, String)> = plugins
        .iter()
        .filter(|plugin| plugin.enabled && plugin.core)
        .filter_map(|plugin| {
            let line = validate_agent_prompt_line(plugin.line.as_deref()?).ok()?;
            Some((plugin.id.as_str(), line))
        })
        .collect();
    if lines.is_empty() {
        return None;
    }
    lines.sort_by(|a, b| a.0.cmp(b.0));
    lines.dedup_by(|a, b| a.0 == b.0);
    let mut section = BUILT_IN_PLUGINS_HEADING.to_string();
    for (_, line) in lines {
        section.push_str("\n- ");
        section.push_str(&line);
    }
    Some(section)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(id: &str, core: bool, enabled: bool, line: &str) -> PluginPromptLine {
        PluginPromptLine {
            id: id.to_string(),
            core,
            enabled,
            line: Some(line.to_string()),
        }
    }

    #[test]
    fn no_contributing_plugin_composes_no_section() {
        assert_eq!(compose_built_in_plugins_section(&[]), None);
        let silent = PluginPromptLine {
            id: "core-q7".into(),
            core: true,
            enabled: true,
            line: None,
        };
        assert_eq!(compose_built_in_plugins_section(&[silent]), None);
    }

    #[test]
    fn only_enabled_core_plugins_contribute() {
        let plugins = [
            plugin("core-q7", true, true, "CORE_LINE_Q7"),
            plugin("third-q7", false, true, "THIRD_LINE_Q7"),
            plugin("off-q7", true, false, "DISABLED_LINE_Q7"),
        ];
        assert_eq!(
            compose_built_in_plugins_section(&plugins).as_deref(),
            Some("Built-in plugins:\n- CORE_LINE_Q7")
        );
    }

    #[test]
    fn lines_are_ordered_by_plugin_id_one_bullet_each() {
        let plugins = [
            plugin("zeta-q7", true, true, "ZETA_LINE"),
            plugin("alpha-q7", true, true, "  ALPHA_LINE  "),
            plugin("alpha-q7", true, true, "ALPHA_DUPLICATE"),
        ];
        assert_eq!(
            compose_built_in_plugins_section(&plugins).as_deref(),
            Some("Built-in plugins:\n- ALPHA_LINE\n- ZETA_LINE")
        );
    }

    #[test]
    fn line_must_be_one_bounded_line() {
        let at_limit = "x".repeat(MAX_AGENT_PROMPT_LINE_CHARS);
        assert_eq!(validate_agent_prompt_line(&at_limit), Ok(at_limit.clone()));
        let multibyte = "\u{2014}".repeat(MAX_AGENT_PROMPT_LINE_CHARS);
        assert!(validate_agent_prompt_line(&multibyte).is_ok());
        for refused in [
            "x".repeat(MAX_AGENT_PROMPT_LINE_CHARS + 1),
            "first\nsecond".to_string(),
            "first\r\nsecond".to_string(),
            "tab\there".to_string(),
            "   ".to_string(),
        ] {
            assert!(validate_agent_prompt_line(&refused).is_err(), "{refused:?}");
            let plugins = [plugin("core-q7", true, true, &refused)];
            assert_eq!(compose_built_in_plugins_section(&plugins), None);
        }
    }
}
