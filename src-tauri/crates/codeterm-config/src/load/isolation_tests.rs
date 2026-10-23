#[test]
#[should_panic(expected = "Ambient user config is forbidden in unit tests")]
fn ambient_user_config_is_refused_before_reading_home() {
    super::load_config();
}
