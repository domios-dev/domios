//! Dev-only hold-time instrumentation for hot locks.
//! Debug builds wrap guards and log `[LOCK-HOLD]` when a guard is held past the
//! threshold; release builds compile to transparent parking_lot wrappers (zero overhead).

use std::time::Duration;

pub const LOCK_HOLD_THRESHOLD: Duration = Duration::from_millis(50);

#[cfg(debug_assertions)]
pub fn should_report(held: Duration) -> bool {
    held >= LOCK_HOLD_THRESHOLD
}

#[cfg(debug_assertions)]
pub mod order;

#[cfg(not(debug_assertions))]
pub mod order {
    #[track_caller]
    pub fn check_blocking_wait(_what: &str) {}
    pub fn violation_count() -> usize {
        0
    }
    pub fn violations_mentioning(_needle: &str) -> Vec<String> {
        Vec::new()
    }
}

#[cfg(debug_assertions)]
mod imp {
    use super::order::{self, Access};
    use super::should_report;
    use parking_lot::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
    use std::ops::{Deref, DerefMut};
    use std::panic::Location;
    use std::time::Duration;
    use std::time::Instant;

    pub struct TimedGuard<G> {
        name: &'static str,
        kind: &'static str,
        start: Instant,
        waited: Duration,
        site: &'static Location<'static>,
        order_token: u64,
        inner: G,
    }

    struct Acquisition {
        name: &'static str,
        kind: &'static str,
        access: Access,
        addr: usize,
        site: &'static Location<'static>,
    }

    impl Acquisition {
        fn blocking<G>(self, take: impl FnOnce() -> G) -> TimedGuard<G> {
            order::check_blocking(self.name, self.addr, self.access, self.site);
            let waiting = Instant::now();
            let inner = take();
            self.acquired(waiting, inner)
        }

        fn bounded<G>(self, take: impl FnOnce() -> Option<G>) -> Option<TimedGuard<G>> {
            order::check_blocking(self.name, self.addr, self.access, self.site);
            self.attempt(take)
        }

        fn attempt<G>(self, take: impl FnOnce() -> Option<G>) -> Option<TimedGuard<G>> {
            let waiting = Instant::now();
            let inner = take()?;
            Some(self.acquired(waiting, inner))
        }

        fn acquired<G>(self, waiting: Instant, inner: G) -> TimedGuard<G> {
            let start = Instant::now();
            TimedGuard {
                name: self.name,
                kind: self.kind,
                start,
                waited: start.duration_since(waiting),
                site: self.site,
                order_token: order::enter(self.name, self.addr, self.access, self.site),
                inner,
            }
        }
    }

    impl<G> TimedGuard<G> {
        pub(super) fn report_held(&self, held: Duration, report: impl FnOnce(String)) {
            if should_report(held) {
                report(format_report(
                    self.name,
                    self.kind,
                    held,
                    self.waited,
                    self.site,
                ));
            }
        }
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
            self.report_held(self.start.elapsed(), |report| log::debug!("{report}"));
            order::exit(self.order_token);
        }
    }

    pub(super) fn format_report(
        name: &str,
        kind: &str,
        held: Duration,
        waited: Duration,
        site: &'static Location<'static>,
    ) -> String {
        format!(
            "[LOCK-HOLD] {}.{}() held {}ms (waited {}ms) at {}:{}",
            name,
            kind,
            held.as_millis(),
            waited.as_millis(),
            site.file(),
            site.line()
        )
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
        #[track_caller]
        fn acquisition(&self, kind: &'static str) -> Acquisition {
            Acquisition {
                name: self.name,
                kind,
                access: Access::Exclusive,
                addr: self as *const Self as usize,
                site: Location::caller(),
            }
        }
        #[track_caller]
        pub fn lock(&self) -> TimedGuard<MutexGuard<'_, T>> {
            self.acquisition("lock").blocking(|| self.inner.lock())
        }
        #[track_caller]
        pub fn try_lock(&self) -> Option<TimedGuard<MutexGuard<'_, T>>> {
            self.acquisition("try_lock")
                .attempt(|| self.inner.try_lock())
        }
        #[track_caller]
        pub fn try_lock_until(&self, deadline: Instant) -> Option<TimedGuard<MutexGuard<'_, T>>> {
            self.acquisition("try_lock_until")
                .bounded(|| self.inner.try_lock_until(deadline))
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
        #[track_caller]
        fn acquisition(&self, kind: &'static str, access: Access) -> Acquisition {
            Acquisition {
                name: self.name,
                kind,
                access,
                addr: self as *const Self as usize,
                site: Location::caller(),
            }
        }
        #[track_caller]
        pub fn read(&self) -> TimedGuard<RwLockReadGuard<'_, T>> {
            self.acquisition("read", Access::Shared)
                .blocking(|| self.inner.read())
        }
        #[track_caller]
        pub fn try_read(&self) -> Option<TimedGuard<RwLockReadGuard<'_, T>>> {
            self.acquisition("try_read", Access::Shared)
                .attempt(|| self.inner.try_read())
        }
        #[track_caller]
        pub fn try_read_until(
            &self,
            deadline: Instant,
        ) -> Option<TimedGuard<RwLockReadGuard<'_, T>>> {
            self.acquisition("try_read_until", Access::Shared)
                .bounded(|| self.inner.try_read_until(deadline))
        }
        #[track_caller]
        pub fn write(&self) -> TimedGuard<RwLockWriteGuard<'_, T>> {
            self.acquisition("write", Access::Exclusive)
                .blocking(|| self.inner.write())
        }
        #[track_caller]
        pub fn try_write(&self) -> Option<TimedGuard<RwLockWriteGuard<'_, T>>> {
            self.acquisition("try_write", Access::Exclusive)
                .attempt(|| self.inner.try_write())
        }
    }
}

#[cfg(not(debug_assertions))]
mod imp {
    use parking_lot::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
    use std::time::Instant;

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
        #[inline(always)]
        pub fn try_lock_until(&self, deadline: Instant) -> Option<MutexGuard<'_, T>> {
            self.0.try_lock_until(deadline)
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
        pub fn try_read_until(&self, deadline: Instant) -> Option<RwLockReadGuard<'_, T>> {
            self.0.try_read_until(deadline)
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
    fn named_mutex_reports_through_guard_hold_seam_without_waiting() {
        let mutex = TimedMutex::new("launch_registry", ());
        let guard = mutex.lock();
        let mut reports = Vec::new();
        guard.report_held(Duration::from_millis(49), |report| reports.push(report));
        assert!(reports.is_empty());
        guard.report_held(LOCK_HOLD_THRESHOLD, |report| reports.push(report));
        assert_eq!(reports.len(), 1);
        assert!(reports[0].starts_with("[LOCK-HOLD] launch_registry.lock() held 50ms (waited "));
        assert!(reports[0].contains(file!()));
    }

    #[test]
    fn lock_hold_report_names_test_location_and_wait() {
        let site = std::panic::Location::caller();
        let report = imp::format_report(
            "pty",
            "lock",
            Duration::from_millis(5961),
            Duration::from_millis(12),
            site,
        );
        assert_eq!(
            report,
            format!(
                "[LOCK-HOLD] pty.lock() held 5961ms (waited 12ms) at {}:{}",
                file!(),
                site.line()
            )
        );
    }

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
