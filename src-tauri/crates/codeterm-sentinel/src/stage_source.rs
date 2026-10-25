use codeterm_blueprint_types::{
    blueprint_pipeline::BlueprintSpec, blueprint_terminal::Config, sentinel_input::InputSpec,
};
use serde::{Deserialize, Serialize};

use super::source::TabBinding;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StageSource {
    Probe(String),
    Observe(TabBinding),
    Input(InputSpec),
    Terminal { binding: TabBinding, config: Config },
    Pipeline(BlueprintSpec),
}

#[derive(Default, Serialize, Deserialize)]
struct SourceFields {
    #[serde(default)]
    probe: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    observe: Option<TabBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    input: Option<InputSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    terminal: Option<Config>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pipeline: Option<BlueprintSpec>,
}

impl StageSource {
    pub fn from_parts(
        probe: String,
        observe: Option<TabBinding>,
        input: Option<InputSpec>,
        terminal: Option<Config>,
        pipeline: Option<BlueprintSpec>,
    ) -> Result<Self, String> {
        match (probe, observe, input, terminal, pipeline) {
            (probe, None, None, None, Some(spec)) if probe.is_empty() => Ok(Self::Pipeline(spec)),
            (probe, None, None, None, None) if !probe.is_empty() => Ok(Self::Probe(probe)),
            (probe, Some(binding), None, None, None) if probe.is_empty() => {
                Ok(Self::Observe(binding))
            }
            (probe, Some(binding @ TabBinding::Tab(_)), None, Some(config), None)
                if probe.is_empty() =>
            {
                Ok(Self::Terminal { binding, config })
            }
            (probe, None, Some(input), None, None) if probe.is_empty() => Ok(Self::Input(input)),
            _ => Err("exactly one Blueprint source is required".into()),
        }
    }

    pub fn probe(&self) -> &String {
        static EMPTY: String = String::new();
        if let Self::Probe(probe) = self {
            probe
        } else {
            &EMPTY
        }
    }

    pub fn observe(&self) -> Option<&TabBinding> {
        match self {
            Self::Observe(binding) | Self::Terminal { binding, .. } => Some(binding),
            _ => None,
        }
    }

    pub fn input(&self) -> Option<&InputSpec> {
        if let Self::Input(input) = self {
            Some(input)
        } else {
            None
        }
    }

    pub fn terminal(&self) -> Option<&Config> {
        if let Self::Terminal { config, .. } = self {
            Some(config)
        } else {
            None
        }
    }

    pub fn pipeline(&self) -> Option<&BlueprintSpec> {
        if let Self::Pipeline(spec) = self {
            Some(spec)
        } else {
            None
        }
    }
}

impl Serialize for StageSource {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        SourceFields {
            probe: self.probe().clone(),
            observe: self.observe().cloned(),
            input: self.input().cloned(),
            terminal: self.terminal().cloned(),
            pipeline: self.pipeline().cloned(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for StageSource {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let fields = SourceFields::deserialize(deserializer)?;
        Self::from_parts(
            fields.probe,
            fields.observe,
            fields.input,
            fields.terminal,
            fields.pipeline,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeterm_blueprint_types::sentinel_input::{DataRef, MediaKind};

    #[test]
    fn stage_source_codec_round_trips_each_variant() {
        let document = serde_json::json!({"schema_version":2,"kind":"pipeline","name":"pipeline","owner":"tab","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":[{"id":"emit","run":{"lit":"echo ready"}}]});
        let runtime =
            codeterm_blueprint_types::blueprint_document::Document::from_definition(&document)
                .unwrap()
                .lower()
                .unwrap();
        let spec: BlueprintSpec = serde_json::from_value(runtime["spec"].clone()).unwrap();
        for source in [
            StageSource::Probe("status".into()),
            StageSource::Observe(TabBinding::Tab("tab".into())),
            StageSource::Input(InputSpec {
                kind: MediaKind::Text,
                data: DataRef::Text("text".into()),
                convert_to: None,
                convert_timeout_ms: None,
            }),
            StageSource::Terminal {
                binding: TabBinding::Tab("tab".into()),
                config: Config {
                    pattern: Some("ready".into()),
                    idle_ms: None,
                },
            },
            StageSource::Pipeline(spec.clone()),
        ] {
            let value = serde_json::to_value(&source).unwrap();
            assert_eq!(
                serde_json::from_value::<StageSource>(value).unwrap(),
                source
            );
        }
        assert!(serde_json::from_value::<StageSource>(
            serde_json::json!({"probe":"status","observe":{"kind":"tab","id":"tab"}})
        )
        .is_err());
    }
}
