#[derive(Debug, PartialEq, Eq)]
pub(super) enum LeaseRead {
    Continue,
    RetireGroup,
}

pub(super) fn lease_read(bytes: isize, interrupted: bool) -> LeaseRead {
    if bytes > 0 || (bytes < 0 && interrupted) {
        LeaseRead::Continue
    } else {
        LeaseRead::RetireGroup
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ownership_requires_a_live_lease() {
        assert_eq!(lease_read(0, false), LeaseRead::RetireGroup);
        assert_eq!(lease_read(-1, false), LeaseRead::RetireGroup);
        assert_eq!(lease_read(-1, true), LeaseRead::Continue);
        assert_eq!(lease_read(1, false), LeaseRead::Continue);
    }
}
