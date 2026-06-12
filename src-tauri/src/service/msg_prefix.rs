//! One builder for cross-pane / report / mesh injection prefixes. Read by the
//! receiving agent and the UI parser (`src/lib/chatPrefixes.ts`).
//!
//!   -=-codeterm:pane label=<Name> pane=<id>[ model=<m>][ mesh=<peer>]-=-body
//!   -=-codeterm:report from=<Name> pane=<id> ...-=-body
//!
//! This wraps the unified `message_markers` frame so pane/report prefixes and
//! bridge attachment markers share one wire family.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtxKind {
    /// A direct message from one pane (or peer) to another.
    Message,
    /// A hierarchy / cron report bubbling up to a parent or inbox.
    Report,
}

#[derive(Debug, Default, Clone)]
pub struct CtxPrefix {
    pub kind_report: bool,
    pub from_name: String,
    pub from_pane: String,
    pub model: Option<String>,
    pub mesh: Option<String>,
}

fn short_id(id: &str) -> &str {
    id.split('-').next().unwrap_or(id)
}

/// Brackets would break the parser's anchoring; strip them from names.
fn clean_name(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '[' && *c != ']')
        .collect::<String>()
        .trim()
        .to_string()
}

impl CtxPrefix {
    pub fn message(from_name: impl Into<String>, from_pane: impl Into<String>) -> Self {
        Self {
            kind_report: false,
            from_name: from_name.into(),
            from_pane: from_pane.into(),
            ..Default::default()
        }
    }

    pub fn report(from_name: impl Into<String>, from_pane: impl Into<String>) -> Self {
        Self {
            kind_report: true,
            from_name: from_name.into(),
            from_pane: from_pane.into(),
            ..Default::default()
        }
    }

    pub fn model(mut self, model: Option<String>) -> Self {
        self.model = model.filter(|m| !m.trim().is_empty());
        self
    }

    pub fn mesh(mut self, mesh: Option<String>) -> Self {
        self.mesh = mesh.filter(|m| !m.trim().is_empty());
        self
    }

    /// Render just the `[...]` prefix token (no trailing space/body).
    pub fn build(&self) -> String {
        let pane = short_id(&self.from_pane);
        let label = clean_name(&self.from_name);
        let kind = if self.kind_report { "report" } else { "pane" };
        let name_key = if self.kind_report { "from" } else { "label" };
        crate::message_markers::Marker::new(kind)
            .attr(name_key, &label)
            .attr("pane", pane)
            .attr_opt("model", self.model.as_deref().map(str::trim))
            .attr_opt("mesh", self.mesh.as_deref().map(str::trim))
            .build()
    }

    /// Prefix marker + the message body.
    pub fn wrap(&self, body: &str) -> String {
        format!("{}{}", self.build(), body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_message_tags_codeterm() {
        let p = CtxPrefix::message("Edison", "e8a0ac20-1111").build();
        assert_eq!(p, "-=-codeterm:pane label=Edison pane=e8a0ac20-=-");
    }

    #[test]
    fn enriched_message_orders_segments() {
        let p = CtxPrefix::message("Edison", "e8a0ac20")
            .model(Some("opus".into()))
            .mesh(Some("Mac-Air".into()))
            .build();
        assert_eq!(
            p,
            "-=-codeterm:pane label=Edison pane=e8a0ac20 model=opus mesh=Mac-Air-=-"
        );
    }

    #[test]
    fn report_uses_report_lead() {
        let p = CtxPrefix::report("Worker", "abc12345")
            .model(Some("sonnet".into()))
            .build();
        assert_eq!(
            p,
            "-=-codeterm:report from=Worker pane=abc12345 model=sonnet-=-"
        );
    }

    #[test]
    fn empty_model_and_mesh_are_dropped() {
        let p = CtxPrefix::message("A", "deadbeef")
            .model(Some("  ".into()))
            .mesh(Some(String::new()))
            .build();
        assert_eq!(p, "-=-codeterm:pane label=A pane=deadbeef-=-");
    }

    #[test]
    fn brackets_in_name_are_stripped() {
        let p = CtxPrefix::message("Main [dev]", "abcdef12").build();
        assert_eq!(p, "-=-codeterm:pane label=Main%20dev pane=abcdef12-=-");
    }

    #[test]
    fn wrap_appends_body() {
        let p = CtxPrefix::message("Edison", "e8a0ac20");
        assert_eq!(
            p.wrap("hello"),
            "-=-codeterm:pane label=Edison pane=e8a0ac20-=-hello"
        );
    }
}
