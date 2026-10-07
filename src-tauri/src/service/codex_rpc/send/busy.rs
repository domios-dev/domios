use super::{Origin, Sidecar};
use std::sync::atomic::Ordering;

pub(super) fn hold(origin: Origin, active: bool) -> bool {
    origin == Origin::User && active
}

pub(crate) fn user_turn_pending(active: bool, admitted: bool) -> bool {
    active || admitted
}

pub(super) struct Admission<'a> {
    sidecar: &'a Sidecar,
    reserved: bool,
}

impl<'a> Admission<'a> {
    pub(super) fn acquire(
        sidecar: &'a Sidecar,
        origin: Origin,
        active: bool,
    ) -> Result<Self, &'static str> {
        if hold(
            origin,
            user_turn_pending(active, sidecar.busy.load(Ordering::Acquire)),
        ) {
            return Err("active RPC turn; user input remains queued");
        }
        let reserved = origin == Origin::User;
        if reserved
            && sidecar
                .busy
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return Err("RPC turn admission pending; user input remains queued");
        }
        Ok(Self { sidecar, reserved })
    }

    pub(super) fn commit(&mut self) {
        self.reserved = false;
    }
}

impl Drop for Admission<'_> {
    fn drop(&mut self) {
        if self.reserved {
            self.sidecar.busy.store(false, Ordering::Release);
        }
    }
}

pub(crate) fn joined_text(parts: impl IntoIterator<Item = String>) -> String {
    parts.into_iter().collect::<Vec<_>>().join("\n\n")
}
