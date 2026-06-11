use super::*;

pub(super) fn last_change_for_install(version: Option<String>, channel: Option<String>) -> LastChange {
    LastChange {
        kind: "installed".to_string(),
        from_version: None,
        to_version: version,
        channel,
        time: now_rfc3339(),
    }
}

pub(super) fn last_change_for_update(
    from_version: Option<String>,
    to_version: Option<String>,
    channel: Option<String>,
) -> LastChange {
    let kind = match (&from_version, &to_version) {
        (Some(from), Some(to)) if version_cmp(to, from).is_lt() => "downgraded",
        _ => "updated",
    };
    LastChange {
        kind: kind.to_string(),
        from_version,
        to_version,
        channel,
        time: now_rfc3339(),
    }
}

pub(super) fn version_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    match (semverish(left), semverish(right)) {
        (Some(a), Some(b)) => a.cmp(&b),
        _ => left.cmp(right),
    }
}

pub(super) fn semverish(value: &str) -> Option<Vec<u64>> {
    value
        .split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect()
}
