use super::UserDrain;

pub(super) enum OwnerWakeBoundary {
    Wake(u64),
    Dispatch(u64),
}

impl OwnerWakeBoundary {
    pub(super) fn after_drain(
        wake_before_drain: u64,
        dispatch_before_drain: u64,
        outcome: &Result<UserDrain, String>,
    ) -> Self {
        match outcome {
            Ok(UserDrain::Idle) => Self::Wake(wake_before_drain),
            Ok(UserDrain::ZeroWriteRefusal {
                dispatch_generation,
            }) => Self::Dispatch(*dispatch_generation),
            Err(_) => Self::Dispatch(dispatch_before_drain),
        }
    }

    pub(super) fn refusal_generation(&self) -> Option<u64> {
        match self {
            Self::Wake(_) => None,
            Self::Dispatch(generation) => Some(*generation),
        }
    }

    pub(super) fn changed(&self, wake: u64, dispatch: u64) -> bool {
        match self {
            Self::Wake(before) => wake != *before,
            Self::Dispatch(before) => dispatch != *before,
        }
    }
}

#[cfg(test)]
mod tests;
