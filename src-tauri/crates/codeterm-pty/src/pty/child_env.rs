use std::path::Path;

const APPIMAGE_LAUNCHER_ENV: &[&str] = &["APPDIR", "APPIMAGE", "ARGV0", "OWD"];

/// Environment edits every child CodeTerm spawns needs on top of what it inherits:
/// `Some` sets a variable, `None` removes it.
pub fn child_env_changes<'a>(
    vars: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<(String, Option<String>)> {
    child_env_changes_with_exclusions(vars, &[])
}

pub fn child_env_changes_with_exclusions<'a>(
    vars: impl IntoIterator<Item = (&'a str, &'a str)>,
    excluded: &[&str],
) -> Vec<(String, Option<String>)> {
    let vars: Vec<(&str, &str)> = vars.into_iter().collect();
    let mut changes = Vec::new();
    // Windows Python otherwise writes the ANSI code page and crashes on Cyrillic or emoji output.
    if cfg!(windows)
        && !vars
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case("PYTHONUTF8"))
    {
        changes.push(("PYTHONUTF8".to_string(), Some("1".to_string())));
    }
    if cfg!(target_os = "linux") {
        changes.extend(appimage_env_changes(&vars));
    }
    changes.extend(
        vars.iter()
            .filter(|(key, _)| {
                excluded
                    .iter()
                    .any(|excluded| key.eq_ignore_ascii_case(excluded))
            })
            .map(|(key, _)| (key.to_string(), None)),
    );
    changes
}

/// Undoes an AppImage launcher's runtime for children: list entries inside the mounted
/// image are dropped, so system binaries never load the bundle's libraries.
pub(crate) fn appimage_env_changes(vars: &[(&str, &str)]) -> Vec<(String, Option<String>)> {
    let value_of = |name: &str| {
        vars.iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| *value)
            .filter(|value| !value.is_empty())
    };
    let (Some(appdir), Some(_)) = (value_of("APPDIR"), value_of("APPIMAGE")) else {
        return Vec::new();
    };
    let appdir = Path::new(appdir);
    let mut changes = Vec::new();
    for (key, value) in vars {
        if APPIMAGE_LAUNCHER_ENV.contains(key) {
            changes.push((key.to_string(), None));
            continue;
        }
        let entries: Vec<&str> = value.split(':').collect();
        let kept: Vec<&str> = entries
            .iter()
            .copied()
            .filter(|entry| entry.is_empty() || !Path::new(entry).starts_with(appdir))
            .collect();
        if kept.len() == entries.len() {
            continue;
        }
        let rest = (!kept.iter().all(|entry| entry.is_empty())).then(|| kept.join(":"));
        changes.push((key.to_string(), rest));
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    const APPDIR: &str = "/tmp/.mount_CodeTeXyz";

    fn changes(vars: &[(&str, &str)]) -> Vec<(String, Option<String>)> {
        let mut changes = appimage_env_changes(vars);
        changes.sort();
        changes
    }

    #[test]
    fn appimage_cleanup_is_inert_outside_an_appimage() {
        let vars = [
            ("APPDIR", APPDIR),
            ("LD_LIBRARY_PATH", "/tmp/.mount_CodeTeXyz/usr/lib"),
        ];
        assert!(
            changes(&vars).is_empty(),
            "APPDIR alone is not proof of an AppImage"
        );
        assert!(changes(&[("PATH", "/usr/bin")]).is_empty());
    }

    #[test]
    fn appimage_cleanup_drops_only_entries_inside_the_mounted_image() {
        let vars = [
            ("APPDIR", APPDIR),
            ("APPIMAGE", "/home/u/CodeTerm.AppImage"),
            ("ARGV0", "CodeTerm.AppImage"),
            ("OWD", "/home/u"),
            (
                "PATH",
                "/tmp/.mount_CodeTeXyz/usr/bin:/usr/local/bin:/usr/bin",
            ),
            (
                "LD_LIBRARY_PATH",
                "/tmp/.mount_CodeTeXyz/usr/lib:/tmp/.mount_CodeTeXyz/lib",
            ),
            (
                "GDK_PIXBUF_MODULE_FILE",
                "/tmp/.mount_CodeTeXyz/usr/lib/loaders.cache",
            ),
            (
                "XDG_DATA_DIRS",
                "/tmp/.mount_CodeTeXyz/usr/share:/usr/share",
            ),
            ("HOME", "/home/u"),
            ("BROWSER_URL", "http://localhost:3000"),
            ("SIBLING", "/tmp/.mount_CodeTeXyzOther/bin"),
        ];
        assert_eq!(
            changes(&vars),
            vec![
                ("APPDIR".to_string(), None),
                ("APPIMAGE".to_string(), None),
                ("ARGV0".to_string(), None),
                ("GDK_PIXBUF_MODULE_FILE".to_string(), None),
                ("LD_LIBRARY_PATH".to_string(), None),
                ("OWD".to_string(), None),
                (
                    "PATH".to_string(),
                    Some("/usr/local/bin:/usr/bin".to_string())
                ),
                ("XDG_DATA_DIRS".to_string(), Some("/usr/share".to_string())),
            ]
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_children_default_python_to_utf8_without_overriding_a_choice() {
        assert_eq!(
            child_env_changes([("PATH", "C:\\Windows")]),
            vec![("PYTHONUTF8".to_string(), Some("1".to_string()))]
        );
        assert!(child_env_changes([("PythonUtf8", "0")]).is_empty());
    }
}
