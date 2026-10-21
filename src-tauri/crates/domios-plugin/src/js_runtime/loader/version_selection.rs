use std::cmp::Ordering;

pub(super) fn system_version_order(system_id: bool, candidate: &str, current: &str) -> Ordering {
    if !system_id {
        return Ordering::Equal;
    }
    match (
        semver::Version::parse(candidate),
        semver::Version::parse(current),
    ) {
        (Ok(candidate), Ok(current)) => candidate.cmp_precedence(&current),
        (Ok(_), Err(_)) => Ordering::Greater,
        (Err(_), Ok(_)) => Ordering::Less,
        (Err(_), Err(_)) => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_provider_selection_preserves_the_newer_version() {
        for (candidate, current, expected) in [
            ("1.14.41", "1.14.6", Ordering::Greater),
            ("1.14.41", "1.14.42", Ordering::Less),
            ("1.14.41", "1.14.41", Ordering::Equal),
            ("1.14.41+checkout", "1.14.41+channel", Ordering::Equal),
            ("1.14.41-rc.1", "1.14.41", Ordering::Less),
            ("invalid", "1.14.41", Ordering::Less),
            ("1.14.41", "invalid", Ordering::Greater),
        ] {
            assert_eq!(system_version_order(true, candidate, current), expected);
        }
    }

    #[test]
    fn third_party_versions_do_not_change_root_priority() {
        assert_eq!(
            system_version_order(false, "1.14.6", "1.14.41"),
            Ordering::Equal
        );
    }
}
