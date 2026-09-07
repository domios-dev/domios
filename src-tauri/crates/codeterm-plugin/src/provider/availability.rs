//! What enumeration observed about a provider's launch binary on THIS machine.
//! Registration says a provider exists; only a probe says it can run.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::js_runtime::{probe_bin, BinResolution};

/// The three answers an observation can give. `Unknown` is what a probe that
/// could not run reports: `Unavailable` means the ladder searched and found
/// nothing, never that we failed to look.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAvailability {
    Available,
    Unavailable,
    #[default]
    Unknown,
}

impl ProviderAvailability {
    /// The whole decision, as a pure function of what the resolver concluded.
    pub fn from_resolution(resolution: &BinResolution) -> Self {
        match resolution {
            BinResolution::Found(_) => Self::Available,
            BinResolution::Missing => Self::Unavailable,
            BinResolution::Unknown => Self::Unknown,
        }
    }

    /// True only for the observed negative. An unanswered probe is not a veto.
    pub fn is_refusable(self) -> bool {
        self == Self::Unavailable
    }
}

/// One provider's launch-binary observation: the verdict, the binary it was
/// about, and the search a refusal has to be able to name.
#[derive(Debug, Clone)]
pub struct LaunchBinaryObservation {
    pub availability: ProviderAvailability,
    pub binary: Option<String>,
    pub searched: Vec<PathBuf>,
}

/// Observe a provider's declared launch binary through the SHARED resolver.
/// A provider that declares none launches nothing external, so nothing gates it.
pub fn observe_launch_binary(binary: Option<&str>) -> LaunchBinaryObservation {
    let Some(binary) = binary.map(str::trim).filter(|b| !b.is_empty()) else {
        return LaunchBinaryObservation {
            availability: ProviderAvailability::Available,
            binary: None,
            searched: Vec::new(),
        };
    };
    let probe = probe_bin(binary);
    LaunchBinaryObservation {
        availability: ProviderAvailability::from_resolution(&probe.resolution),
        binary: Some(binary.to_string()),
        searched: probe.searched,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_is_three_valued_over_the_resolver_verdict() {
        let table = [
            (
                BinResolution::Found(PathBuf::from("/usr/local/bin/codex")),
                ProviderAvailability::Available,
            ),
            (BinResolution::Missing, ProviderAvailability::Unavailable),
            (BinResolution::Unknown, ProviderAvailability::Unknown),
        ];
        for (resolution, expected) in table {
            assert_eq!(
                ProviderAvailability::from_resolution(&resolution),
                expected,
                "{resolution:?}"
            );
        }
    }

    /// The row this slice exists for: a resolver that could not answer must not
    /// be read as a machine that answered no.
    #[test]
    fn a_probe_that_could_not_run_is_not_unavailable() {
        let verdict = ProviderAvailability::from_resolution(&BinResolution::Unknown);
        assert_ne!(verdict, ProviderAvailability::Unavailable);
        assert_ne!(verdict, ProviderAvailability::Available);
        assert!(!verdict.is_refusable());
    }

    #[test]
    fn only_the_observed_negative_is_refusable() {
        assert!(ProviderAvailability::Unavailable.is_refusable());
        assert!(!ProviderAvailability::Available.is_refusable());
        assert!(!ProviderAvailability::Unknown.is_refusable());
    }

    #[test]
    fn a_provider_declaring_no_launch_binary_is_not_gated_on_one() {
        for declared in [None, Some(""), Some("   ")] {
            let observed = observe_launch_binary(declared);
            assert_eq!(observed.availability, ProviderAvailability::Available);
            assert_eq!(observed.binary, None);
        }
    }

    #[test]
    fn a_declared_binary_that_cannot_be_resolved_reads_unavailable() {
        let observed = observe_launch_binary(Some("codeterm-provider-binary-that-cannot-exist"));
        assert_eq!(
            observed.binary.as_deref(),
            Some("codeterm-provider-binary-that-cannot-exist")
        );
        assert_ne!(observed.availability, ProviderAvailability::Available);
        assert!(
            !observed.searched.is_empty(),
            "a refusal has to be able to name where it looked"
        );
    }

    /// `unknown` is a wire state of its own; a reader that round-trips it as
    /// `unavailable` would silently re-introduce the collapse.
    #[test]
    fn availability_round_trips_all_three_states_on_the_wire() {
        for (verdict, wire) in [
            (ProviderAvailability::Available, "\"available\""),
            (ProviderAvailability::Unavailable, "\"unavailable\""),
            (ProviderAvailability::Unknown, "\"unknown\""),
        ] {
            assert_eq!(serde_json::to_string(&verdict).unwrap(), wire);
            assert_eq!(
                serde_json::from_str::<ProviderAvailability>(wire).unwrap(),
                verdict
            );
        }
        assert_eq!(
            ProviderAvailability::default(),
            ProviderAvailability::Unknown,
            "a descriptor nothing observed must not claim availability"
        );
    }
}
