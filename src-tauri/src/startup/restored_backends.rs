use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

pub(super) fn ordered_ids(mut tabs: Vec<(String, bool, Option<u64>)>) -> Vec<String> {
    tabs.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then_with(|| right.2.cmp(&left.2))
            .then_with(|| left.0.cmp(&right.0))
    });
    tabs.into_iter().map(|tab| tab.0).collect()
}

pub(super) fn run_wave<T: Sync>(
    items: &[T],
    limit: usize,
    mut validate: impl FnMut(&T) -> Result<(), String>,
    materialize: impl Fn(&T) -> Result<(), String> + Sync,
) -> Vec<Result<(), String>> {
    assert!(limit > 0);
    let checked = items.iter().map(&mut validate).collect::<Vec<_>>();
    let ready = checked
        .iter()
        .enumerate()
        .filter_map(|(index, result)| result.is_ok().then_some(index))
        .collect::<Vec<_>>();
    let results = Mutex::new(
        checked
            .into_iter()
            .map(|result| result.err().map(Err))
            .collect::<Vec<_>>(),
    );
    let cursor = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..limit.min(ready.len()) {
            scope.spawn(|| loop {
                let slot = cursor.fetch_add(1, Ordering::Relaxed);
                let Some(&index) = ready.get(slot) else {
                    break;
                };
                let result = materialize(&items[index]);
                results.lock().unwrap()[index] = Some(result);
            });
        }
    });
    results
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|result| result.expect("every validated tab was materialized"))
        .collect()
}

#[cfg(test)]
mod tests;
