//! Pre-teardown viability probe. Proves an update can reach install BEFORE any
//! destructive PTY teardown, so a doomed update aborts while panes are alive.

use std::path::{Path, PathBuf};

/// Real probe for the plugin (in-place) install path: verified payload present,
/// staging dir writable (where the installer payload lands), relaunch binary
/// resolvable. All checks are non-destructive — nothing here kills a pane.
///
/// We probe the staging temp dir, NOT the final install target: the installer
/// (NSIS/AppImage) may elevate to write a privileged target, so a non-elevated
/// write to `Program Files` would be a false negative and abort a viable update.
pub fn plugin_install_viability(verified_len: usize) -> Result<(), String> {
    let relaunch = std::env::current_exe().map_err(|e| e.to_string());
    let stageable = probe_writable(&std::env::temp_dir());
    evaluate_viability(verified_len, relaunch, stageable)
}

/// Pure decision seam: given the collected inputs, decide whether teardown may
/// proceed. Split out so the ordering/abort logic is unit-testable without a
/// filesystem or a running app.
pub(crate) fn evaluate_viability(
    verified_len: usize,
    relaunch_path: Result<PathBuf, String>,
    staging_writable: Result<(), String>,
) -> Result<(), String> {
    if verified_len == 0 {
        return Err("viability: verified installer payload is empty".to_string());
    }
    relaunch_path.map_err(|e| format!("viability: relaunch path unresolved: {e}"))?;
    staging_writable.map_err(|e| format!("viability: staging dir not writable: {e}"))?;
    Ok(())
}

fn probe_writable(dir: &Path) -> Result<(), String> {
    let probe = dir.join(format!(".codeterm-update-viability-{}", std::process::id()));
    std::fs::write(&probe, b"ok").map_err(|e| format!("{}: {e}", dir.display()))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_payload_is_never_viable() {
        let err = evaluate_viability(0, Ok(PathBuf::from("/x")), Ok(())).unwrap_err();
        assert!(err.contains("payload is empty"));
    }

    #[test]
    fn unresolved_relaunch_path_aborts_before_teardown() {
        let err = evaluate_viability(10, Err("no current exe".to_string()), Ok(())).unwrap_err();
        assert!(err.contains("relaunch path unresolved"));
        assert!(err.contains("no current exe"));
    }

    #[test]
    fn unwritable_staging_dir_aborts_before_teardown() {
        let err = evaluate_viability(
            10,
            Ok(PathBuf::from("/x/codeterm")),
            Err("Permission denied".to_string()),
        )
        .unwrap_err();
        assert!(err.contains("staging dir not writable"));
        assert!(err.contains("Permission denied"));
    }

    #[test]
    fn all_inputs_ok_is_viable() {
        assert!(evaluate_viability(10, Ok(PathBuf::from("/x/codeterm")), Ok(())).is_ok());
    }
}
