//! Dev-only hold-time instrumentation for the ordered hot locks.
//! Debug builds wrap guards and log `[LOCK-HOLD]` when a guard is held past the
//! threshold; release builds compile to transparent parking_lot wrappers (zero overhead).

use std::time::Duration;

pub const LOCK_HOLD_THRESHOLD: Duration = Duration::from_millis(50);

#[cfg(debug_assertions)]
pub fn should_report(held: Duration) -> bool {
    held >= LOCK_HOLD_THRESHOLD
}

#[cfg(debug_assertions)]
mod imp {
    use super::{should_report, LOCK_HOLD_THRESHOLD};
    use parking_lot::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
    use std::ops::{Deref, DerefMut};
    use std::time::Instant;

    pub struct TimedGuard<G> {
        name: &'static str,
        kind: &'static str,
        start: Instant,
        inner: G,
    }

    impl<G: Deref> Deref for TimedGuard<G> {
        type Target = G::Target;
        fn deref(&self) -> &Self::Target {
            &self.inner
        }
    }

    impl<G: DerefMut> DerefMut for TimedGuard<G> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.inner
        }
    }

    impl<G> Drop for TimedGuard<G> {
        fn drop(&mut self) {
            let held = self.start.elapsed();
            if should_report(held) {
                log::warn!(
                    "[LOCK-HOLD] {}.{}() held {}ms (threshold {}ms)",
                    self.name,
                    self.kind,
                    held.as_millis(),
                    LOCK_HOLD_THRESHOLD.as_millis()
                );
            }
        }
    }

    pub struct TimedMutex<T> {
        name: &'static str,
        inner: Mutex<T>,
    }

    impl<T> TimedMutex<T> {
        pub const fn new(name: &'static str, val: T) -> Self {
            Self {
                name,
                inner: Mutex::new(val),
            }
        }
        pub fn lock(&self) -> TimedGuard<MutexGuard<'_, T>> {
            let inner = self.inner.lock();
            TimedGuard {
                name: self.name,
                kind: "lock",
                start: Instant::now(),
                inner,
            }
        }
        pub fn try_lock(&self) -> Option<TimedGuard<MutexGuard<'_, T>>> {
            self.inner.try_lock().map(|inner| TimedGuard {
                name: self.name,
                kind: "try_lock",
                start: Instant::now(),
                inner,
            })
        }
    }

    pub struct TimedRwLock<T> {
        name: &'static str,
        inner: RwLock<T>,
    }

    impl<T> TimedRwLock<T> {
        pub const fn new(name: &'static str, val: T) -> Self {
            Self {
                name,
                inner: RwLock::new(val),
            }
        }
        pub fn read(&self) -> TimedGuard<RwLockReadGuard<'_, T>> {
            let inner = self.inner.read();
            TimedGuard {
                name: self.name,
                kind: "read",
                start: Instant::now(),
                inner,
            }
        }
        pub fn try_read(&self) -> Option<TimedGuard<RwLockReadGuard<'_, T>>> {
            self.inner.try_read().map(|inner| TimedGuard {
                name: self.name,
                kind: "try_read",
                start: Instant::now(),
                inner,
            })
        }
        pub fn write(&self) -> TimedGuard<RwLockWriteGuard<'_, T>> {
            let inner = self.inner.write();
            TimedGuard {
                name: self.name,
                kind: "write",
                start: Instant::now(),
                inner,
            }
        }
        pub fn try_write(&self) -> Option<TimedGuard<RwLockWriteGuard<'_, T>>> {
            self.inner.try_write().map(|inner| TimedGuard {
                name: self.name,
                kind: "try_write",
                start: Instant::now(),
                inner,
            })
        }
    }
}

#[cfg(not(debug_assertions))]
mod imp {
    use parking_lot::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

    #[repr(transparent)]
    pub struct TimedMutex<T>(Mutex<T>);

    impl<T> TimedMutex<T> {
        pub const fn new(_name: &'static str, val: T) -> Self {
            Self(Mutex::new(val))
        }
        #[inline(always)]
        pub fn lock(&self) -> MutexGuard<'_, T> {
            self.0.lock()
        }
        #[inline(always)]
        pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
            self.0.try_lock()
        }
    }

    #[repr(transparent)]
    pub struct TimedRwLock<T>(RwLock<T>);

    impl<T> TimedRwLock<T> {
        pub const fn new(_name: &'static str, val: T) -> Self {
            Self(RwLock::new(val))
        }
        #[inline(always)]
        pub fn read(&self) -> RwLockReadGuard<'_, T> {
            self.0.read()
        }
        #[inline(always)]
        pub fn try_read(&self) -> Option<RwLockReadGuard<'_, T>> {
            self.0.try_read()
        }
        #[inline(always)]
        pub fn write(&self) -> RwLockWriteGuard<'_, T> {
            self.0.write()
        }
        #[inline(always)]
        pub fn try_write(&self) -> Option<RwLockWriteGuard<'_, T>> {
            self.0.try_write()
        }
    }
}

pub use imp::{TimedMutex, TimedRwLock};

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::*;

    #[test]
    fn threshold_boundary() {
        assert!(!should_report(Duration::from_millis(0)));
        assert!(!should_report(Duration::from_millis(49)));
        assert!(should_report(LOCK_HOLD_THRESHOLD));
        assert!(should_report(Duration::from_millis(50)));
        assert!(should_report(Duration::from_millis(500)));
    }

    #[test]
    fn guard_derefs_to_inner() {
        let m = TimedMutex::new("test", 41u32);
        {
            let mut g = m.lock();
            *g += 1;
        }
        assert_eq!(*m.lock(), 42);

        let rw = TimedRwLock::new("test", vec![1, 2, 3]);
        assert_eq!(rw.read().len(), 3);
        rw.write().push(4);
        assert_eq!(rw.read().len(), 4);
    }
}
