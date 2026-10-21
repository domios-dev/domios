pub const MAX_CANDIDATES: usize = 10;

pub fn selection(scores: &[f64]) -> Vec<usize> {
    let mut order: Vec<_> = (0..scores.len()).collect();
    order.sort_by(|&a, &b| scores[a].total_cmp(&scores[b]).then(a.cmp(&b)));
    order.truncate(MAX_CANDIDATES);
    order
}

pub fn skip_reason(scores: &[f64]) -> Option<&'static str> {
    if scores.len() <= 3 {
        return Some("small_pool");
    }
    if scores.iter().any(|score| !score.is_finite()) {
        return None;
    }
    let order = selection(scores);
    let top = scores[order[0]];
    let second = scores[order[1]];
    let gap = second - top;
    (top.is_finite()
        && second.is_finite()
        && gap >= 0.25
        && gap >= top.abs().max(second.abs()) * 0.5)
        .then_some("decisive_ranking")
}

#[cfg(test)]
mod tests;
