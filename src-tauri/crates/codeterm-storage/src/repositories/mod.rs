pub mod cron;
pub mod desk;
pub mod sentinel;
mod team_teardown;
pub mod workspaces;

pub use team_teardown::{TeamTeardown, TeamTeardownApplied};

use crate::error::StorageError;
use crate::owner::{lock, on_owner_thread};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

/// Opaque, ordered store commit revision. Callers only obtain it from receipts and reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Revision(pub(crate) u64);

impl fmt::Display for Revision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "r{}", self.0)
    }
}

/// Compare-and-set expectation for one entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    Absent,
    At(Revision),
}

/// Bounded latency metadata of one committed operation; never carries data bodies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommitTimings {
    pub queue_wait: Duration,
    pub transaction: Duration,
    pub commit: Duration,
    pub group_size: u32,
}

/// Durability acknowledgment: issued only after SQLite COMMIT succeeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitReceipt {
    pub revision: Revision,
    pub rows_written: u64,
    pub timings: CommitTimings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Committed<T> {
    pub value: T,
    pub receipt: CommitReceipt,
}

/// A value read inside one short read transaction at the given committed revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read<T> {
    pub value: T,
    pub revision: Revision,
}

pub(crate) fn expect_matches(
    entity: &str,
    expect: Expect,
    actual: Option<Revision>,
) -> Result<(), StorageError> {
    let expected = match expect {
        Expect::Absent => None,
        Expect::At(revision) => Some(revision),
    };
    if expected == actual {
        return Ok(());
    }
    Err(StorageError::StaleRevision {
        entity: entity.to_string(),
        expected,
        actual,
    })
}

pub(crate) fn to_sql_u64(field: &'static str, value: u64) -> Result<i64, StorageError> {
    i64::try_from(value)
        .map_err(|_| StorageError::invalid(field, "exceeds the signed 64-bit range"))
}

pub(crate) fn from_sql_u64(field: &'static str, value: i64) -> Result<u64, StorageError> {
    u64::try_from(value).map_err(|_| StorageError::Corrupt {
        detail: format!("negative {field}"),
    })
}

/// Settled once the owner released the database, its handles and the root claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShutdownReceipt {
    pub drained_writes: u64,
    pub revision: Revision,
    pub checkpointed: bool,
}

/// Bounded counters only; no operation bodies.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageMetrics {
    pub committed_operations: u64,
    pub failed_operations: u64,
    pub commit_groups: u64,
    pub rejected_admissions: u64,
    pub max_queue_wait: Duration,
    pub max_transaction: Duration,
    pub max_commit: Duration,
    pub last_group: Option<CommitTimings>,
}

enum SlotState<T> {
    Pending(Option<Waker>),
    Ready(Result<T, StorageError>),
    Taken,
}

struct Slot<T> {
    state: Mutex<SlotState<T>>,
    ready: Condvar,
}

/// Result of one accepted or refused Storage operation. Await it on a runtime; only
/// already-off-runtime callers may block on it, and only with a bounded deadline.
pub struct Completion<T> {
    slot: Arc<Slot<T>>,
}

/// Writer-side handle; dropping it unsettled reports the outcome as unconfirmed.
pub(crate) struct Settle<T> {
    slot: Option<Arc<Slot<T>>>,
}

pub(crate) fn completion<T>() -> (Settle<T>, Completion<T>) {
    let slot = Arc::new(Slot {
        state: Mutex::new(SlotState::Pending(None)),
        ready: Condvar::new(),
    });
    (
        Settle {
            slot: Some(slot.clone()),
        },
        Completion { slot },
    )
}

fn fill<T>(slot: &Slot<T>, result: Result<T, StorageError>) {
    let waker = match std::mem::replace(&mut *lock(&slot.state), SlotState::Ready(result)) {
        SlotState::Pending(waker) => waker,
        _ => None,
    };
    slot.ready.notify_all();
    if let Some(waker) = waker {
        waker.wake();
    }
}

impl<T> Settle<T> {
    pub(crate) fn settle(mut self, result: Result<T, StorageError>) {
        if let Some(slot) = self.slot.take() {
            fill(&slot, result);
        }
    }
}

impl<T> Drop for Settle<T> {
    fn drop(&mut self) {
        if let Some(slot) = self.slot.take() {
            fill(
                &slot,
                Err(StorageError::Unconfirmed {
                    operation: "storage operation",
                }),
            );
        }
    }
}

impl<T> Completion<T> {
    pub(crate) fn settled(result: Result<T, StorageError>) -> Self {
        let (settle, completion) = completion();
        settle.settle(result);
        completion
    }

    pub fn is_settled(&self) -> bool {
        matches!(*lock(&self.slot.state), SlotState::Ready(_))
    }

    /// Non-blocking: the outcome if settled, otherwise the still-pending completion.
    pub fn try_take(self) -> Result<Result<T, StorageError>, Self> {
        let taken = {
            let mut state = lock(&self.slot.state);
            match std::mem::replace(&mut *state, SlotState::Taken) {
                SlotState::Ready(result) => Some(result),
                other => {
                    *state = other;
                    None
                }
            }
        };
        taken.ok_or(self)
    }

    /// Bounded blocking adapter for already-off-runtime callers. A deadline expiry is
    /// `Unconfirmed`, never a failure: the operation may still commit.
    pub fn wait(self, deadline: Duration) -> Result<T, StorageError> {
        if on_owner_thread() {
            return Err(StorageError::Reentrant);
        }
        let until = Instant::now()
            .checked_add(deadline)
            .ok_or_else(|| StorageError::invalid("deadline", "overflows"))?;
        let mut state = lock(&self.slot.state);
        loop {
            match std::mem::replace(&mut *state, SlotState::Taken) {
                SlotState::Ready(result) => return result,
                SlotState::Taken => {
                    return Err(StorageError::Unconfirmed {
                        operation: "completion already taken",
                    })
                }
                pending => *state = pending,
            }
            let now = Instant::now();
            if now >= until {
                return Err(StorageError::Unconfirmed {
                    operation: "storage completion",
                });
            }
            state = match self.slot.ready.wait_timeout(state, until - now) {
                Ok((guard, _)) => guard,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
    }
}

impl<T> Future for Completion<T> {
    type Output = Result<T, StorageError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = lock(&self.slot.state);
        match std::mem::replace(&mut *state, SlotState::Taken) {
            SlotState::Ready(result) => Poll::Ready(result),
            SlotState::Pending(_) => {
                *state = SlotState::Pending(Some(cx.waker().clone()));
                Poll::Pending
            }
            SlotState::Taken => Poll::Ready(Err(StorageError::Unconfirmed {
                operation: "completion already taken",
            })),
        }
    }
}
