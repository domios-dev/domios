use std::sync::Mutex;

use codeterm_service_core::service::restore_admission::{
    changes, Admission, AdmissionPermit, Priority,
};

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
    pub chat_foreground: usize,
    pub chat_background: usize,
}

struct Pending {
    indices: Vec<usize>,
    idle_reserved: usize,
}

const CHAT_BACKGROUND_BUDGET: usize = 3;

pub(super) const RESTORE_BUDGETS: RestoreBudgets = RestoreBudgets {
    foreground: 1,
    background: 4,
    chat_foreground: 1,
    chat_background: CHAT_BACKGROUND_BUDGET,
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
    validate: impl Fn(&T) -> Result<(), String> + Sync,
    materialize: impl Fn(&T) -> Result<(), String> + Sync,
) -> Vec<Result<(), String>> {
    run_transport_waves(
        items,
        RestoreBudgets {
            foreground: 0,
            background: limit,
            chat_foreground: 0,
            chat_background: 1,
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
        |item, _, ()| materialize(item),
    )
}

pub(super) fn run_transport_waves<T: Sync, Validated>(
    items: &[T],
    budgets: RestoreBudgets,
    transport: impl Fn(&T) -> RestoreTransport,
    priorities: impl Fn() -> Vec<Priority> + Sync,
    validate: impl Fn(&T) -> Result<Validated, String> + Sync,
    materialize: impl Fn(&T, Admission, Validated) -> Result<(), String> + Sync,
) -> Vec<Result<(), String>> {
    assert!(budgets.background > 0 && budgets.chat_background > 0);
    let mut queued = [Vec::new(), Vec::new()];
    for (index, item) in items.iter().enumerate() {
        queued[transport(item).index()].push(index);
    }
    let results = Mutex::new(vec![None; items.len()]);
    let [pty, chat] = queued;
    let pending = [
        Mutex::new(Pending {
            indices: pty,
            idle_reserved: budgets.foreground,
        }),
        Mutex::new(Pending {
            indices: chat,
            idle_reserved: budgets.chat_foreground,
        }),
    ];
    std::thread::scope(|scope| {
        let priorities = &priorities;
        let validate = &validate;
        let materialize = &materialize;
        let results = &results;
        for (kind, limit, foreground_only) in [
            (RestoreTransport::Pty, budgets.foreground, true),
            (RestoreTransport::Pty, budgets.background, false),
            (RestoreTransport::Chat, budgets.chat_foreground, true),
            (RestoreTransport::Chat, budgets.chat_background, false),
        ] {
            let pending = &pending[kind.index()];
            let workers = limit.min(pending.lock().unwrap().indices.len());
            for _ in 0..workers {
                scope.spawn(move || loop {
                    let observed = changes().revision();
                    let index = {
                        let mut pending = pending.lock().unwrap();
                        if pending.indices.is_empty() {
                            break;
                        }
                        let priorities = priorities();
                        take_prioritized(&mut pending, &priorities, foreground_only).map(|index| {
                            if foreground_only {
                                pending.idle_reserved -= 1;
                            }
                            (
                                index,
                                Admission {
                                    priority: priorities[index],
                                    permit: if foreground_only {
                                        AdmissionPermit::Reserved
                                    } else {
                                        AdmissionPermit::Shared
                                    },
                                },
                            )
                        })
                    };
                    let Some((index, admission)) = index else {
                        changes().wait(observed);
                        continue;
                    };
                    changes().notify();
                    let item = &items[index];
                    let result = validate(item)
                        .and_then(|validated| materialize(item, admission, validated));
                    results.lock().unwrap()[index] = Some(result);
                    if foreground_only {
                        pending.lock().unwrap().idle_reserved += 1;
                    }
                    changes().notify();
                });
            }
        }
    });
    results
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|result| result.expect("every queued tab was processed"))
        .collect()
}

fn take_prioritized(
    pending: &mut Pending,
    priorities: &[Priority],
    foreground_only: bool,
) -> Option<usize> {
    let reserve_foreground = !foreground_only && pending.idle_reserved > 0;
    let position = pending
        .indices
        .iter()
        .enumerate()
        .filter(|(_, index)| {
            let foreground = priorities[**index] != Priority::Background;
            (!foreground_only || foreground) && (!reserve_foreground || !foreground)
        })
        .min_by_key(|(_, index)| priorities[**index])
        .map(|(position, _)| position)?;
    Some(pending.indices.remove(position))
}

#[cfg(test)]
mod tests;
