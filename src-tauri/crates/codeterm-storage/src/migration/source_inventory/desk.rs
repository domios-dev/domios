use super::SourceRole;

pub(super) fn scan_dirs() -> Vec<String> {
    vec!["sessions".to_string(), "sessions/backups".to_string()]
}

pub(super) fn classify(dir: &str, name: &str) -> Option<SourceRole> {
    match dir {
        "sessions" => {
            if name == "last.json" || name == "agent-session-identities.json" {
                return Some(SourceRole::Current);
            }
            if name == "agent-session-identities.json.bak" {
                return Some(SourceRole::RotatingBackup);
            }
            if name == "agent-session-identities.json.tmp"
                || name == "last.json.tmp"
                || name.starts_with("last.json.tmp.")
            {
                return Some(SourceRole::InterruptedWrite);
            }
            if name.starts_with("last.json.corrupt-") || name.starts_with("last.json.reduced-") {
                return Some(SourceRole::Quarantined);
            }
            if name.ends_with(".json") && name != "agent-session-identities.json" {
                return Some(SourceRole::NamedSnapshot);
            }
            name.starts_with("last.json.")
                .then_some(SourceRole::Unrecognized)
        }
        "sessions/backups" => Some(if name.ends_with(".json") {
            SourceRole::RotatingBackup
        } else {
            SourceRole::Unrecognized
        }),
        _ => None,
    }
}

pub(super) fn valid_inventory_path(role: SourceRole, relative: &str) -> bool {
    let parts: Vec<&str> = relative.split('/').collect();
    let (dir, name) = match parts.as_slice() {
        ["sessions", name] => ("sessions", *name),
        ["sessions", "backups", name] => ("sessions/backups", *name),
        _ => return false,
    };
    if name.is_empty() || name == "." || name == ".." || name.contains('\\') || name.contains(':') {
        return false;
    }
    classify(dir, name) == Some(role)
}
