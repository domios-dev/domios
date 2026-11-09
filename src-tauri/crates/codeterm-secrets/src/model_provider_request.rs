use crate::SecretString;
use codeterm_plugin::model_providers::{EndpointKind, ModelProviderDefinition};
use serde::Deserialize;

#[derive(Deserialize, ts_rs::TS)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelProviderRequest {
    PresetAdd {
        preset: codeterm_plugin::model_providers::ModelPreset,
    },
    PresetRemove {
        name: String,
    },
    PresetList,
    Add {
        provider: ModelProviderDefinition,
        #[serde(deserialize_with = "secret_from_wire")]
        #[ts(as = "String")]
        value: SecretString,
    },
    AddFromSecret {
        provider: ModelProviderDefinition,
        source_secret: String,
    },
    Update {
        provider: ModelProviderDefinition,
        #[serde(default, deserialize_with = "optional_secret_from_wire")]
        #[ts(optional, as = "Option<String>")]
        value: Option<SecretString>,
    },
    Discover {
        kind: EndpointKind,
        base_url: String,
        #[serde(default, deserialize_with = "optional_secret_from_wire")]
        #[ts(optional, as = "Option<String>")]
        value: Option<SecretString>,
        #[serde(default)]
        #[ts(optional)]
        source_secret: Option<String>,
    },
    Remove {
        name: String,
    },
    List,
    Test {
        name: String,
    },
}

fn secret_from_wire<'de, D: serde::Deserializer<'de>>(d: D) -> Result<SecretString, D::Error> {
    String::deserialize(d).map(SecretString::from)
}

fn optional_secret_from_wire<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<SecretString>, D::Error> {
    Option::<String>::deserialize(d).map(|value| value.map(SecretString::from))
}
