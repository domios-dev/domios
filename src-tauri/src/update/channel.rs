use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::checker::GITHUB_REPO;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Stable,
    Rc,
    Nightly,
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Stable => "stable",
            Self::Rc => "rc",
            Self::Nightly => "nightly",
        };
        f.write_str(s)
    }
}

impl FromStr for Channel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "stable" => Ok(Self::Stable),
            "rc" => Ok(Self::Rc),
            "nightly" => Ok(Self::Nightly),
            other => Err(format!("unknown channel: {}", other)),
        }
    }
}

pub fn manifest_filename(c: Channel) -> &'static str {
    match c {
        Channel::Stable => "latest.json",
        Channel::Rc => "latest-rc.json",
        Channel::Nightly => "latest-nightly.json",
    }
}

pub fn default_endpoints(c: Channel) -> Vec<String> {
    vec![format!(
        "https://github.com/{}/releases/latest/download/{}",
        GITHUB_REPO,
        manifest_filename(c)
    )]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_channels() {
        assert_eq!("stable".parse::<Channel>().unwrap(), Channel::Stable);
        assert_eq!("rc".parse::<Channel>().unwrap(), Channel::Rc);
        assert_eq!("nightly".parse::<Channel>().unwrap(), Channel::Nightly);
        assert_eq!("STABLE".parse::<Channel>().unwrap(), Channel::Stable);
        assert_eq!(" Rc ".parse::<Channel>().unwrap(), Channel::Rc);
    }

    #[test]
    fn rejects_unknown_channel() {
        assert!("beta".parse::<Channel>().is_err());
        assert!("".parse::<Channel>().is_err());
    }

    #[test]
    fn default_is_stable() {
        assert_eq!(Channel::default(), Channel::Stable);
    }

    #[test]
    fn display_round_trips() {
        for c in [Channel::Stable, Channel::Rc, Channel::Nightly] {
            assert_eq!(c.to_string().parse::<Channel>().unwrap(), c);
        }
    }

    #[test]
    fn manifest_filename_mapping() {
        assert_eq!(manifest_filename(Channel::Stable), "latest.json");
        assert_eq!(manifest_filename(Channel::Rc), "latest-rc.json");
        assert_eq!(manifest_filename(Channel::Nightly), "latest-nightly.json");
    }

    #[test]
    fn default_endpoints_format() {
        let stable = default_endpoints(Channel::Stable);
        assert_eq!(stable.len(), 1);
        assert_eq!(
            stable[0],
            "https://github.com/rollacode/codeterm/releases/latest/download/latest.json"
        );

        let rc = default_endpoints(Channel::Rc);
        assert!(rc[0].ends_with("/latest-rc.json"));

        let nightly = default_endpoints(Channel::Nightly);
        assert!(nightly[0].ends_with("/latest-nightly.json"));
    }

    #[test]
    fn serde_lowercase_roundtrip() {
        let json = serde_json::to_string(&Channel::Nightly).unwrap();
        assert_eq!(json, "\"nightly\"");
        let back: Channel = serde_json::from_str("\"rc\"").unwrap();
        assert_eq!(back, Channel::Rc);
    }
}
