use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Off,
    Console,
    Amplitude,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Zone {
    #[default]
    Us,
    Eu,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Project {
    #[default]
    Test,
    Prod,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub mode: Mode,
    pub zone: Zone,
    pub project: Project,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        Self::parse(
            std::env::var("DOMIOS_ANALYTICS_MODE").ok().as_deref(),
            std::env::var("DOMIOS_AMPLITUDE_SERVER_ZONE")
                .ok()
                .as_deref(),
            std::env::var("DOMIOS_ANALYTICS_PROJECT").ok().as_deref(),
        )
    }

    pub fn parse(
        mode: Option<&str>,
        zone: Option<&str>,
        project: Option<&str>,
    ) -> Result<Self, String> {
        Ok(Self {
            mode: match mode.unwrap_or("off") {
                "off" => Mode::Off,
                "console" => Mode::Console,
                "amplitude" => Mode::Amplitude,
                _ => return Err("invalid analytics mode".into()),
            },
            zone: match zone.unwrap_or("us") {
                "us" | "US" => Zone::Us,
                "eu" | "EU" => Zone::Eu,
                _ => return Err("invalid analytics server zone".into()),
            },
            project: match project.unwrap_or("test") {
                "test" => Project::Test,
                "prod" => Project::Prod,
                _ => return Err("invalid analytics project".into()),
            },
        })
    }
}

pub mod amplitude;
pub mod console;
pub mod fake;
pub mod local;
pub mod off;

use crate::{AnalyticsSink, SecretReader};

pub fn from_config(
    config: Config,
    secrets: SecretReader,
) -> Result<Box<dyn AnalyticsSink>, String> {
    match config.mode {
        Mode::Off => Ok(Box::new(off::Off)),
        Mode::Console => Ok(Box::new(console::Console)),
        Mode::Amplitude => Ok(Box::new(local::Local::new(config, secrets))),
    }
}
