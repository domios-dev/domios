use codeterm_team_package::contracts::EffortDecision;

/// Applies the requested effort, or the nearest level the model offers on the provider's declared scale.
pub(super) fn clamp(
    model: &str,
    requested: &str,
    levels: &[String],
    scale: &[String],
    default: Option<&str>,
) -> EffortDecision {
    if levels.iter().any(|level| level == requested) {
        return EffortDecision {
            requested: requested.to_string(),
            applied: requested.to_string(),
            reason: None,
        };
    }
    let rank = |effort: &str| scale.iter().position(|level| level == effort);
    let nearest = rank(requested).and_then(|wanted| {
        levels
            .iter()
            .filter_map(|level| rank(level.as_str()).map(|position| (position, level)))
            .min_by_key(|(position, _)| (position.abs_diff(wanted), *position))
            .map(|(_, level)| level.as_str())
    });
    let applied = nearest
        .or_else(|| default.filter(|default| levels.iter().any(|level| level.as_str() == *default)))
        .unwrap_or(levels[0].as_str());
    EffortDecision {
        requested: requested.to_string(),
        applied: applied.to_string(),
        reason: Some(format!(
            "{model} does not offer {requested} effort; using {applied}"
        )),
    }
}
