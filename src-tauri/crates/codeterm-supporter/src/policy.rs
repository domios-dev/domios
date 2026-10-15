pub const IGNORE_CONFIDENCE_FLOOR: f64 = 0.60;
pub const OWNER_NOTIFICATION_CONFIDENCE: f64 = 0.80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfidenceBand {
    Strong,
    Weak,
    Ignored,
}

pub fn confidence_band(confidence: f64) -> ConfidenceBand {
    if !(0.0..=1.0).contains(&confidence) {
        return ConfidenceBand::Ignored;
    }
    if confidence >= OWNER_NOTIFICATION_CONFIDENCE {
        ConfidenceBand::Strong
    } else if confidence >= IGNORE_CONFIDENCE_FLOOR {
        ConfidenceBand::Weak
    } else {
        ConfidenceBand::Ignored
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tab_state_band_mapping() {
        assert_eq!(confidence_band(0.80), ConfidenceBand::Strong);
        assert_eq!(confidence_band(0.79), ConfidenceBand::Weak);
        assert_eq!(confidence_band(0.60), ConfidenceBand::Weak);
        assert_eq!(confidence_band(0.59), ConfidenceBand::Ignored);
    }
}
