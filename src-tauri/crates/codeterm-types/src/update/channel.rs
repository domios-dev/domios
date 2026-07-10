use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

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
    fn serde_lowercase_roundtrip() {
        let json = serde_json::to_string(&Channel::Nightly).unwrap();
        assert_eq!(json, "\"nightly\"");
        let back: Channel = serde_json::from_str("\"rc\"").unwrap();
        assert_eq!(back, Channel::Rc);
    }
}
