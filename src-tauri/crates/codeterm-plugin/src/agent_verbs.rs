//! What an installed plugin lets agents do, as data: the verbs reachable through
//! `codeterm plugin <id> <verb>`, plus the mem skill item that makes them findable
//! by `codeterm mem search`. Pure — the host owns the store and the triggers.

use serde::Serialize;

/// `source` of every plugin skill item; the suffix is the plugin id.
pub const PLUGIN_SKILL_SOURCE_PREFIX: &str = "plugin-agent-verbs:";
/// `contributions_changed` kind fired when the agent-command registry may have changed.
pub const AGENT_COMMANDS_CONTRIBUTION_KIND: &str = "agent_commands";
pub const PLUGIN_SKILL_TAG: &str = "plugin-agent-verbs";
const CONFIG_HELP_VERBS_MARKER: &str = "Agent verbs:";
const AGENT_GUIDE_HEADING: &str = "\n## Agent guide\n\n";
const MAX_KEYWORDS: usize = 24;
const MIN_KEYWORD_LEN: usize = 3;
const STOPWORDS: &[&str] = &[
    "and", "the", "for", "with", "from", "into", "onto", "your", "their", "its", "this", "that",
    "via", "per", "any", "all", "are", "can", "use", "uses", "using", "plugin", "codeterm",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentVerbCatalog {
    pub id: String,
    pub display_name: String,
    pub description: Option<String>,
    /// Usage shapes such as `send <chat-id> [--key <k>] <text>`.
    pub verbs: Vec<String>,
    pub keywords: Vec<String>,
    /// The plugin's bounded `agentGuide` markdown, when it ships one. Kept out of
    /// the `codeterm capabilities` listing; the mem item and `plugin get` carry it.
    #[serde(skip)]
    pub guide: Option<String>,
    /// Core status decided by the loader; only core catalogs reach every agent.
    #[serde(skip)]
    pub core: bool,
    /// The validated `agentPromptLine`; it reaches prompts only when `core`.
    #[serde(skip)]
    pub prompt_line: Option<String>,
}

impl AgentVerbCatalog {
    /// Declared verbs win; otherwise the `Agent verbs:` sentence of `configHelp`
    /// is read. Declared keywords win; otherwise words of the name and description.
    pub fn compose(
        id: &str,
        display_name: &str,
        description: Option<&str>,
        declared_verbs: &[String],
        config_help: Option<&str>,
        declared_keywords: &[String],
    ) -> Self {
        let verbs = if declared_verbs.is_empty() {
            config_help.map(verbs_from_config_help).unwrap_or_default()
        } else {
            clean_list(declared_verbs.iter().map(String::as_str))
        };
        let description = description
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .map(str::to_string);
        let mut keywords = if declared_keywords.is_empty() {
            words_of(&[display_name, description.as_deref().unwrap_or("")])
        } else {
            clean_list(declared_keywords.iter().map(|k| k.as_str()))
                .into_iter()
                .map(|k| k.to_lowercase())
                .collect()
        };
        for verb in verbs.iter().filter_map(|v| verb_name(v)) {
            keywords.push(verb.to_lowercase());
        }
        let mut seen = std::collections::HashSet::new();
        keywords.retain(|k| seen.insert(k.clone()));
        keywords.truncate(MAX_KEYWORDS);
        Self {
            id: id.to_string(),
            display_name: display_name.to_string(),
            description,
            verbs,
            keywords,
            guide: None,
            core: false,
            prompt_line: None,
        }
    }

    pub fn with_guide(mut self, guide: Option<String>) -> Self {
        self.guide = guide
            .map(|g| g.trim().to_string())
            .filter(|g| !g.is_empty());
        self
    }

    pub fn with_prompt_line(mut self, core: bool, prompt_line: Option<String>) -> Self {
        self.core = core;
        self.prompt_line = prompt_line;
        self
    }

    pub fn is_core(&self) -> bool {
        self.core
    }

    pub fn invocation(&self) -> String {
        format!("codeterm plugin {} <verb>", self.id)
    }
}

fn clean_list<'a>(items: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    items
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter(|s| seen.insert(s.to_string()))
        .map(str::to_string)
        .collect()
}

fn verb_name(usage: &str) -> Option<&str> {
    usage.split_whitespace().next()
}

fn words_of(texts: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for text in texts {
        for word in text.split(|c: char| !c.is_alphanumeric()) {
            let word = word.to_lowercase();
            if word.chars().count() >= MIN_KEYWORD_LEN
                && !word.chars().all(|c| c.is_ascii_digit())
                && !STOPWORDS.contains(&word.as_str())
            {
                out.push(word);
            }
        }
    }
    out
}

/// Backticked spans of the sentence that follows `Agent verbs:`, up to the first
/// period outside a span.
pub fn verbs_from_config_help(help: &str) -> Vec<String> {
    let Some(start) = help.find(CONFIG_HELP_VERBS_MARKER) else {
        return Vec::new();
    };
    let rest = &help[start + CONFIG_HELP_VERBS_MARKER.len()..];
    let mut spans = Vec::new();
    let mut in_span = false;
    let mut current = String::new();
    for c in rest.chars() {
        match (c, in_span) {
            ('`', false) => in_span = true,
            ('`', true) => {
                spans.push(std::mem::take(&mut current));
                in_span = false;
            }
            (c, true) => current.push(c),
            ('.', false) => break,
            _ => {}
        }
    }
    clean_list(spans.iter().map(String::as_str))
}

/// One mem skill item per plugin, keyed by a stable title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSkillItem {
    pub plugin_id: String,
    pub title: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub body: String,
    pub source: String,
}

pub fn plugin_skill_title(plugin_id: &str) -> String {
    format!("plugin-{plugin_id}")
}

pub fn plugin_skill_source(plugin_id: &str) -> String {
    format!("{PLUGIN_SKILL_SOURCE_PREFIX}{plugin_id}")
}

pub fn compose_plugin_skill(catalog: &AgentVerbCatalog) -> PluginSkillItem {
    let id = &catalog.id;
    let name = &catalog.display_name;
    let mut body = format!("# {name} plugin (`{id}`)\n\n");
    if let Some(description) = &catalog.description {
        body.push_str(description);
        body.push_str("\n\n");
    }
    body.push_str(&format!(
        "Installed CodeTerm plugin. Agents call it with `{}`; setup state and full guidance: `codeterm plugin get {id}`.\n\n",
        catalog.invocation()
    ));
    if catalog.verbs.is_empty() {
        body.push_str("Verbs: none declared in the manifest.\n");
    } else {
        body.push_str("Verbs:\n");
        for verb in &catalog.verbs {
            body.push_str(&format!("- `codeterm plugin {id} {verb}`\n"));
        }
    }
    if !catalog.keywords.is_empty() {
        body.push_str(&format!("\nKeywords: {}\n", catalog.keywords.join(", ")));
    }
    if let Some(guide) = &catalog.guide {
        body.push_str(AGENT_GUIDE_HEADING);
        body.push_str(guide);
        body.push('\n');
    }

    let mut tags = vec![
        "plugin".to_string(),
        PLUGIN_SKILL_TAG.to_string(),
        id.clone(),
    ];
    tags.extend(catalog.keywords.iter().cloned());
    let mut seen = std::collections::HashSet::new();
    tags.retain(|t| seen.insert(t.clone()));

    PluginSkillItem {
        plugin_id: id.clone(),
        title: plugin_skill_title(id),
        summary: format!("{name} plugin: run `{}`", catalog.invocation()),
        tags,
        body,
        source: plugin_skill_source(id),
    }
}

/// A plugin skill item already in mem, found by its source prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingPluginSkill {
    pub mem_id: String,
    pub title: String,
    pub source: String,
    pub body: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PluginSkillPlan {
    pub upserts: Vec<PluginSkillItem>,
    /// Mem ids of items whose plugin no longer exposes agent verbs.
    pub removals: Vec<String>,
}

/// Reconcile desired against stored: unchanged items are left alone, so a repeat
/// pass with the same catalogs plans nothing.
pub fn plan_plugin_skill_sync(
    catalogs: &[AgentVerbCatalog],
    existing: &[ExistingPluginSkill],
) -> PluginSkillPlan {
    let mut desired: Vec<PluginSkillItem> = catalogs.iter().map(compose_plugin_skill).collect();
    desired.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
    desired.dedup_by(|a, b| a.plugin_id == b.plugin_id);

    let removals = existing
        .iter()
        .filter(|item| {
            !desired
                .iter()
                .any(|d| d.source == item.source && d.title == item.title)
        })
        .map(|item| item.mem_id.clone())
        .collect();
    let upserts = desired
        .into_iter()
        .filter(|d| {
            !existing.iter().any(|item| {
                item.source == d.source
                    && item.title == d.title
                    && item.body.trim() == d.body.trim()
            })
        })
        .collect();
    PluginSkillPlan { upserts, removals }
}

#[cfg(test)]
mod tests;
