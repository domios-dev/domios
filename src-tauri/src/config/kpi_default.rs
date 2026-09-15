use super::schema::AppConfig;

/// A config saved before KPI defaulted on carries the old default `enabled = false`
/// and no marker. Apply the on-by-default policy once; an opt-out saved afterwards sticks.
pub(super) fn migrate(config: &mut AppConfig) -> bool {
    if config.kpi.default_applied {
        return false;
    }
    config.kpi.enabled = true;
    config.kpi.default_applied = true;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(contents: &str) -> AppConfig {
        toml::from_str(contents).unwrap()
    }

    #[test]
    fn pre_default_config_turns_kpi_on_once() {
        let mut config = parse("[kpi]\nenabled = false\n");
        assert!(!config.kpi.default_applied);
        assert!(migrate(&mut config));
        assert!(config.kpi.enabled);
        assert!(config.kpi.default_applied);
        assert!(!migrate(&mut config));
    }

    #[test]
    fn opt_out_saved_after_the_default_applied_is_kept() {
        let mut config = parse("[kpi]\nenabled = false\ndefault_applied = true\n");
        assert!(!migrate(&mut config));
        assert!(!config.kpi.enabled);
    }

    #[test]
    fn fresh_config_needs_no_migration() {
        let mut config = AppConfig::default();
        assert!(!migrate(&mut config));
        assert!(config.kpi.enabled);
        let reparsed = parse(&toml::to_string_pretty(&config).unwrap());
        assert!(reparsed.kpi.default_applied);
    }
}
