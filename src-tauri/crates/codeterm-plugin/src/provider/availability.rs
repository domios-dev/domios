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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchInterpreter {
    Direct,
    Command,
    PowerShell,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedLaunchSpec {
    pub spec: codeterm_types::agent::LaunchSpec,
    pub interpreter: LaunchInterpreter,
}

pub fn resolve_launch_spec(
    spec: &codeterm_types::agent::LaunchSpec,
) -> Result<ResolvedLaunchSpec, String> {
    let target = match probe_bin(&spec.program).resolution {
        BinResolution::Found(path) => path,
        BinResolution::Missing => {
            return Err(format!("launch program not found: {}", spec.program))
        }
        BinResolution::Unknown => {
            return Err(format!(
                "launch program resolution unavailable: {}",
                spec.program
            ));
        }
    };
    let powershell = || {
        ["pwsh", "powershell"]
            .into_iter()
            .find_map(|program| match probe_bin(program).resolution {
                BinResolution::Found(path) => Some(path),
                _ => None,
            })
    };
    resolve_launch_target(spec, &target, cfg!(windows), powershell)
}

fn resolve_launch_target(
    spec: &codeterm_types::agent::LaunchSpec,
    target: &std::path::Path,
    windows: bool,
    powershell: impl FnOnce() -> Option<PathBuf>,
) -> Result<ResolvedLaunchSpec, String> {
    let mut resolved = spec.clone();
    let target = target.to_string_lossy().into_owned();
    let extension = std::path::Path::new(&target)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let (program, mut prefix, interpreter) = match (windows, extension.as_str()) {
        (true, "cmd" | "bat") => (
            "cmd.exe".to_string(),
            vec!["/d".into(), "/c".into(), target],
            LaunchInterpreter::Command,
        ),
        (true, "ps1") => (
            powershell()
                .ok_or("PowerShell interpreter unavailable for launch script")?
                .to_string_lossy()
                .into_owned(),
            vec![
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-File".into(),
                target,
            ],
            LaunchInterpreter::PowerShell,
        ),
        _ => (target, Vec::new(), LaunchInterpreter::Direct),
    };
    prefix.extend(spec.args.clone());
    resolved.program = program;
    resolved.args = prefix;
    Ok(ResolvedLaunchSpec {
        spec: resolved,
        interpreter,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_spec_resolution_matrix_preserves_argument_and_environment_boundaries() {
        use codeterm_types::agent::LaunchSpec;
        let original = LaunchSpec {
            program: "fixture".into(),
            args: vec!["two words".into(), "юникод 😀 $x; `command`".into()],
            env_set: [("KEY".into(), "raw value".into())].into(),
            env_unset: vec!["INHERITED".into()],
        };
        for (windows, path, expected) in [
            (
                true,
                "C:/program files/fixture.CMD",
                LaunchInterpreter::Command,
            ),
            (true, "C:/tools/fixture.bat", LaunchInterpreter::Command),
            (true, "C:/tools/fixture.ps1", LaunchInterpreter::PowerShell),
            (true, "C:/tools/fixture.exe", LaunchInterpreter::Direct),
            (true, "C:/tools/fixture", LaunchInterpreter::Direct),
            (false, "/usr/bin/fixture", LaunchInterpreter::Direct),
            (false, "/tmp/shebang-script", LaunchInterpreter::Direct),
            (false, "/tmp/fixture.ps1", LaunchInterpreter::Direct),
        ] {
            let resolved =
                resolve_launch_target(&original, std::path::Path::new(path), windows, || {
                    Some(PathBuf::from("C:/tools/pwsh.exe"))
                })
                .unwrap();
            assert_eq!(resolved.interpreter, expected);
            assert!(resolved.spec.args.ends_with(&original.args));
            assert_eq!(resolved.spec.env_set, original.env_set);
            assert_eq!(resolved.spec.env_unset, original.env_unset);
            match expected {
                LaunchInterpreter::Command => {
                    assert_eq!(resolved.spec.program, "cmd.exe");
                    assert_eq!(&resolved.spec.args[..3], &["/d", "/c", path]);
                }
                LaunchInterpreter::PowerShell => {
                    assert_eq!(resolved.spec.program, "C:/tools/pwsh.exe");
                    assert_eq!(
                        &resolved.spec.args[..6],
                        &[
                            "-NoProfile",
                            "-NonInteractive",
                            "-ExecutionPolicy",
                            "Bypass",
                            "-File",
                            path
                        ]
                    );
                }
                LaunchInterpreter::Direct => {
                    assert_eq!(resolved.spec.program, path);
                    assert_eq!(resolved.spec.args, original.args);
                }
            }
        }
    }

    #[test]
    fn launch_spec_script_interpreter_failure_is_explicit() {
        let spec = codeterm_types::agent::LaunchSpec::default();
        assert!(
            resolve_launch_target(&spec, std::path::Path::new("script.ps1"), true, || None)
                .is_err()
        );
        assert!(resolve_launch_target(
            &spec,
            std::path::Path::new("program.exe"),
            true,
            || panic!("not a script")
        )
        .is_ok());
    }

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
