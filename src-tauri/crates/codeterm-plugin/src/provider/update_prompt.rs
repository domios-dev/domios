use super::ProviderUpdateAvailability;

pub(crate) fn availability(screen: &str) -> Option<ProviderUpdateAvailability> {
    let mut pairs = screen.lines().filter_map(|line| {
        let (left, right) = line.split_once(" -> ").or_else(|| line.split_once(" → "))?;
        let current = left.split_whitespace().next_back()?.trim_start_matches('v');
        let latest = right.split_whitespace().next()?.trim_start_matches('v');
        if !version(current) || !version(latest) || current == latest {
            return None;
        }
        Some(ProviderUpdateAvailability {
            current_version: current.into(),
            latest_version: latest.into(),
        })
    });
    let pair = pairs.next()?;
    pairs.next().is_none().then_some(pair)
}

fn version(value: &str) -> bool {
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}
