pub(crate) const MAX_CANDIDATES: usize = 10;

pub(crate) fn selection(scores: &[f64]) -> Vec<usize> {
    let mut order: Vec<_> = (0..scores.len()).collect();
    order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]).then(a.cmp(&b)));
    order.truncate(MAX_CANDIDATES);
    order
}

pub(crate) fn skip_reason(scores: &[f64]) -> Option<&'static str> {
    if scores.len() <= 3 {
        return Some("small_pool");
    }
    let order = selection(scores);
    let top = scores[order[0]];
    let second = scores[order[1]];
    let gap = top - second;
    (top.is_finite()
        && second.is_finite()
        && gap >= 0.25
        && gap >= top.abs().max(second.abs()) * 0.5)
        .then_some("decisive_ranking")
}

#[cfg(test)]
mod tests;
