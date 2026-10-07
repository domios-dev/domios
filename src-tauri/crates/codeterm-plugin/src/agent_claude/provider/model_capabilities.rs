use crate::provider::{ModelInfo, ReasoningEffortInfo};
use std::collections::BTreeMap;

pub(super) const FLAG: &str = "--codeterm-model-capabilities";
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Profile {
    pub efforts: BTreeMap<String, String>,
    pub default_effort: Option<String>,
    pub context_window_tokens: u64,
}
#[derive(serde::Deserialize)]
struct Manifest {
    profiles: BTreeMap<String, Profile>,
    models: BTreeMap<String, String>,
}
fn manifest() -> &'static Manifest {
    static DATA: std::sync::OnceLock<Manifest> = std::sync::OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../../../../../plugins/provider-claude/model-capabilities.json"
        ))
        .expect("Claude model capabilities parse")
    })
}
pub(super) fn profile(model: &str) -> Option<&'static Profile> {
    let data = manifest();
    data.profiles.get(data.models.get(model)?)
}
pub(super) fn hydrate(mut model: ModelInfo) -> ModelInfo {
    if let Some(profile) = profile(&model.id) {
        let order = ["low", "medium", "high", "xhigh", "max"];
        model.reasoning_efforts = Some(
            order
                .into_iter()
                .filter(|id| profile.efforts.contains_key(*id))
                .map(|id| ReasoningEffortInfo {
                    id: id.into(),
                    display_name: id.into(),
                    description: None,
                })
                .collect(),
        );
        model.default_reasoning_effort = profile.default_effort.clone();
    }
    model
}
pub(super) fn effort_args(model: Option<&str>, effort: &str) -> Vec<String> {
    match model {
        Some(model) => profile(model)
            .and_then(|p| p.efforts.get(effort))
            .map(|mapped| vec!["--effort".into(), mapped.clone()])
            .unwrap_or_default(),
        None if !effort.is_empty() => vec!["--effort".into(), effort.into()],
        None => Vec::new(),
    }
}
pub(super) fn compact_env(args: &[String]) -> Option<String> {
    if !args.iter().any(|arg| arg == FLAG) {
        return None;
    }
    let mut model = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--model" {
            model = iter.next().map(String::as_str);
        } else if let Some(value) = arg.strip_prefix("--model=") {
            model = Some(value);
        }
    }
    Some(profile(model?)?.context_window_tokens.to_string())
}
