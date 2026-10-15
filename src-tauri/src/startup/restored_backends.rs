use std::sync::Mutex;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RestoreTransport {
    Pty,
    Chat,
}

impl RestoreTransport {
    fn index(self) -> usize {
        match self {
            Self::Pty => 0,
            Self::Chat => 1,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct RestoreBudgets {
    pub pty: usize,
    pub chat: usize,
}

pub(super) const RESTORE_BUDGETS: RestoreBudgets = RestoreBudgets { pty: 4, chat: 2 };

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

#[cfg(test)]
fn run_wave<T: Sync>(
    items: &[T],
    limit: usize,
    preferred: impl Fn() -> Option<usize> + Sync,
    validate: impl FnMut(&T) -> Result<(), String>,
    materialize: impl Fn(&T) -> Result<(), String> + Sync,
) -> Vec<Result<(), String>> {
    run_transport_waves(
        items,
        RestoreBudgets {
            pty: limit,
            chat: 1,
        },
        |_| RestoreTransport::Pty,
        preferred,
        validate,
        materialize,
    )
}

pub(super) fn run_transport_waves<T: Sync>(
    items: &[T],
    budgets: RestoreBudgets,
    transport: impl Fn(&T) -> RestoreTransport,
    preferred: impl Fn() -> Option<usize> + Sync,
    mut validate: impl FnMut(&T) -> Result<(), String>,
    materialize: impl Fn(&T) -> Result<(), String> + Sync,
) -> Vec<Result<(), String>> {
    assert!(budgets.pty > 0 && budgets.chat > 0);
    let checked = items.iter().map(&mut validate).collect::<Vec<_>>();
    let mut ready = [Vec::new(), Vec::new()];
    for (index, result) in checked.iter().enumerate() {
        if result.is_ok() {
            ready[transport(&items[index]).index()].push(index);
        }
    }
    let results = Mutex::new(
        checked
            .into_iter()
            .map(|result| result.err().map(Err))
            .collect::<Vec<_>>(),
    );
    let pending = ready.map(Mutex::new);
    std::thread::scope(|scope| {
        let preferred = &preferred;
        let materialize = &materialize;
        let results = &results;
        for (kind, limit) in [
            (RestoreTransport::Pty, budgets.pty),
            (RestoreTransport::Chat, budgets.chat),
        ] {
            let pending = &pending[kind.index()];
            let workers = limit.min(pending.lock().unwrap().len());
            for _ in 0..workers {
                scope.spawn(move || loop {
                    let preferred = preferred();
                    let index = take_next(&mut pending.lock().unwrap(), preferred);
                    let Some(index) = index else {
                        break;
                    };
                    let result = materialize(&items[index]);
                    results.lock().unwrap()[index] = Some(result);
                });
            }
        }
    });
    results
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|result| result.expect("every validated tab was materialized"))
        .collect()
}

fn take_next(pending: &mut Vec<usize>, preferred: Option<usize>) -> Option<usize> {
    if pending.is_empty() {
        return None;
    }
    let position = preferred
        .and_then(|preferred| pending.iter().position(|index| *index == preferred))
        .unwrap_or(0);
    Some(pending.remove(position))
}

#[cfg(test)]
mod tests;
