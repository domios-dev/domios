use std::sync::Mutex;

use codeterm_service_core::service::restore_admission::{changes, Priority};

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
    pub foreground: usize,
    pub background: usize,
    pub chat: usize,
}

pub(super) const RESTORE_BUDGETS: RestoreBudgets = RestoreBudgets {
    foreground: 1,
    background: 4,
    chat: 2,
};

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
            foreground: 0,
            background: limit,
            chat: 1,
        },
        |_| RestoreTransport::Pty,
        || {
            let focused = preferred();
            (0..items.len())
                .map(|index| {
                    if Some(index) == focused {
                        Priority::Focused
                    } else {
                        Priority::Background
                    }
                })
                .collect()
        },
        validate,
        materialize,
    )
}

pub(super) fn run_transport_waves<T: Sync>(
    items: &[T],
    budgets: RestoreBudgets,
    transport: impl Fn(&T) -> RestoreTransport,
    priorities: impl Fn() -> Vec<Priority> + Sync,
    mut validate: impl FnMut(&T) -> Result<(), String>,
    materialize: impl Fn(&T) -> Result<(), String> + Sync,
) -> Vec<Result<(), String>> {
    assert!(budgets.background > 0 && budgets.chat > 0);
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
        let priorities = &priorities;
        let materialize = &materialize;
        let results = &results;
        for (kind, limit, foreground_only) in [
            (RestoreTransport::Pty, budgets.foreground, true),
            (RestoreTransport::Pty, budgets.background, false),
            (RestoreTransport::Chat, budgets.chat, false),
        ] {
            let pending = &pending[kind.index()];
            let workers = limit.min(pending.lock().unwrap().len());
            for _ in 0..workers {
                scope.spawn(move || loop {
                    let observed = changes().revision();
                    let index = {
                        let mut pending = pending.lock().unwrap();
                        if pending.is_empty() {
                            break;
                        }
                        take_prioritized(&mut pending, &priorities(), foreground_only)
                    };
                    let Some(index) = index else {
                        changes().wait(observed);
                        continue;
                    };
                    changes().notify();
                    let result = materialize(&items[index]);
                    results.lock().unwrap()[index] = Some(result);
                    changes().notify();
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

fn take_prioritized(
    pending: &mut Vec<usize>,
    priorities: &[Priority],
    foreground_only: bool,
) -> Option<usize> {
    let position = pending
        .iter()
        .enumerate()
        .filter(|(_, index)| !foreground_only || priorities[**index] != Priority::Background)
        .min_by_key(|(_, index)| priorities[**index])
        .map(|(position, _)| position)?;
    Some(pending.remove(position))
}

#[cfg(test)]
fn take_next(pending: &mut Vec<usize>, preferred: Option<usize>) -> Option<usize> {
    let len = pending.iter().copied().max().map_or(0, |index| index + 1);
    let priorities = (0..len)
        .map(|index| {
            if Some(index) == preferred {
                Priority::Focused
            } else {
                Priority::Background
            }
        })
        .collect::<Vec<_>>();
    take_prioritized(pending, &priorities, false)
}

#[cfg(test)]
mod tests;
