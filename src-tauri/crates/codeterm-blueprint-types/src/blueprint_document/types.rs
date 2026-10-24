use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum ValueForm {
    Lit { lit: Value },
    Input { input: String },
    Bind { bind: String },
}
impl ValueForm {
    pub fn literal(value: impl Into<Value>) -> Self {
        Self::Lit { lit: value.into() }
    }
    pub fn input(name: impl Into<String>) -> Self {
        Self::Input { input: name.into() }
    }
    pub fn value(&self) -> Option<&Value> {
        match self {
            Self::Lit { lit } => Some(lit),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Port {
    #[serde(default, rename = "type", skip_serializing_if = "String::is_empty")]
    pub kind: String,
    #[serde(default, rename = "$ref", skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, Port>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<String>,
    #[serde(
        default,
        rename = "additionalProperties",
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_properties: Option<bool>,
    #[serde(default, rename = "enum", skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<Port>>,
    #[serde(default, rename = "maxLength", skip_serializing_if = "Option::is_none")]
    pub max_length: Option<usize>,
    #[serde(default, rename = "x-media", skip_serializing_if = "Option::is_none")]
    pub media: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
pub type Input = Port;
impl Port {
    pub fn accepts_with_types(&self, value: &Value, types: &BTreeMap<String, Port>) -> bool {
        fn check(port: &Port, value: &Value, types: &BTreeMap<String, Port>, depth: usize) -> bool {
            if depth > 32 {
                return false;
            }
            if let Some(reference) = &port.reference {
                return types
                    .get(reference)
                    .is_some_and(|p| check(p, value, types, depth + 1));
            }
            port.accepts(value)
                && value.as_object().is_none_or(|fields| {
                    fields.iter().all(|(key, value)| {
                        port.properties
                            .get(key)
                            .is_none_or(|p| check(p, value, types, depth + 1))
                    })
                })
                && value.as_array().is_none_or(|items| {
                    port.items
                        .as_ref()
                        .is_none_or(|p| items.iter().all(|value| check(p, value, types, depth + 1)))
                })
        }
        check(self, value, types, 0)
    }
    pub fn accepts(&self, value: &Value) -> bool {
        if self
            .choices
            .as_ref()
            .is_some_and(|choices| !choices.contains(value))
        {
            return false;
        }
        let kind = match self.kind.as_str() {
            "" => true,
            "string" => value.is_string(),
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "object" => value.is_object(),
            "array" => value.is_array(),
            "null" => value.is_null(),
            _ => false,
        };
        kind && value
            .as_str()
            .is_none_or(|s| self.max_length.is_none_or(|n| s.chars().count() <= n))
            && value.as_array().is_none_or(|xs| {
                self.items
                    .as_ref()
                    .is_none_or(|p| xs.iter().all(|v| p.accepts(v)))
            })
            && value.as_object().is_none_or(|fields| {
                self.required.iter().all(|k| fields.contains_key(k))
                    && fields.iter().all(|(k, v)| {
                        self.properties
                            .get(k)
                            .map_or(self.additional_properties != Some(false), |p| p.accepts(v))
                    })
            })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Pipeline,
    Watch,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Schedule,
    Event,
    Input,
    Probe,
    Observe,
    Terminal,
    TextInput,
    Command,
    Http,
    Github,
    Filter,
    Convert,
    Agent,
    Ask,
    Notify,
    Send,
    Cron,
    Blueprint,
    RespondPrompt,
    Escalate,
    Stop,
}
impl NodeKind {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Schedule => "schedule",
            Self::Event => "event",
            Self::Input => "input",
            Self::Probe => "probe",
            Self::Observe => "observe",
            Self::Terminal => "terminal",
            Self::TextInput => "text_input",
            Self::Command => "command",
            Self::Http => "http",
            Self::Github => "github",
            Self::Filter => "filter",
            Self::Convert => "convert",
            Self::Agent => "agent",
            Self::Ask => "ask",
            Self::Notify => "notify",
            Self::Send => "send",
            Self::Cron => "cron",
            Self::Blueprint => "blueprint",
            Self::RespondPrompt => "respond_prompt",
            Self::Escalate => "escalate",
            Self::Stop => "stop",
        }
    }
    pub fn effect(&self) -> bool {
        matches!(
            self,
            Self::Notify
                | Self::Send
                | Self::Cron
                | Self::Blueprint
                | Self::RespondPrompt
                | Self::Escalate
                | Self::Stop
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Trigger {
    Schedule {
        every: ValueForm,
    },
    Event {
        kind: ValueForm,
        #[serde(default)]
        params: BTreeMap<String, ValueForm>,
    },
    Input {
        kind: ValueForm,
        source: ValueForm,
    },
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mem_mb: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_bytes: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Python,
    Ts,
    Shell,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Code {
    pub lang: Language,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caps: Vec<String>,
    #[serde(default)]
    pub limits: Limits,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Call {
    #[serde(rename = "ref")]
    pub reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin: Option<super::calls::CallPin>,
    #[serde(default, rename = "with", skip_serializing_if = "BTreeMap::is_empty")]
    pub arguments: BTreeMap<String, ValueForm>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub does: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uses: Option<NodeKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<ValueForm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<Code>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call: Option<Call>,
    #[serde(default, rename = "with", skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, ValueForm>,
    #[serde(default, rename = "in", skip_serializing_if = "Option::is_none")]
    pub input: Option<Port>,
    #[serde(default, rename = "out", skip_serializing_if = "Option::is_none")]
    pub output: Option<Port>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[schemars(
        description = "Data bindings: use in alone for a single value, or named ports on code and command nodes for an object input. Every producer must execute on every path reaching this node."
    )]
    pub bind: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buckets: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub matches: BTreeMap<String, ValueForm>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub routes: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outcomes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caps: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<Limits>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub every: Option<ValueForm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<ValueForm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rearm_on_change: Vec<String>,
    #[serde(default)]
    pub model_classification: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escalation_timeout_ms: Option<u64>,
    #[serde(default)]
    pub session: bool,
    #[serde(default)]
    pub paused: bool,
}
impl Node {
    pub fn kind(&self) -> &str {
        if self.run.is_some() {
            "command"
        } else if self.code.is_some() {
            "code"
        } else if self.call.is_some() {
            "call"
        } else {
            self.uses.as_ref().map_or("", NodeKind::name)
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum ResultTarget {
    Fixed(String),
    Input { input: String },
}
impl ResultTarget {
    pub fn fixed(&self) -> Option<&str> {
        match self {
            Self::Fixed(value) => Some(value),
            Self::Input { .. } => None,
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResultPort {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<ResultTarget>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub port: Option<Port>,
    #[serde(default, rename = "from", skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubBlueprint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchors: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, Input>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub types: BTreeMap<String, Port>,
    pub steps: Vec<Node>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other: Vec<Node>,
    pub result: ResultPort,
}
