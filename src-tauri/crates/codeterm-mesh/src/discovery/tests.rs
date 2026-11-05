use super::parse_first_ip;

const TAILNET_V4: &str = concat!("100.", "65.0.40");

#[test]
fn parse_first_ip_trims_whitespace() {
    for input in [
        concat!("100.", "65.0.40\n"),
        concat!("   100.", "65.0.40   \n"),
    ] {
        assert_eq!(parse_first_ip(input).as_deref(), Some(TAILNET_V4));
    }
}

#[test]
fn parse_first_ip_picks_first_of_many() {
    assert_eq!(
        parse_first_ip(concat!("100.", "65.0.40\nfd7a:", "115c:a1e0::1\n")).as_deref(),
        Some(TAILNET_V4)
    );
}

#[test]
fn parse_first_ip_rejects_gui_bridge_error_string() {
    let raw = "The Tailscale GUI failed to start: The operation couldn't be completed. (Tailscale.CLIError error 3.)\n";
    assert_eq!(parse_first_ip(raw), None);
}

#[test]
fn parse_first_ip_rejects_empty() {
    assert_eq!(parse_first_ip(""), None);
    assert_eq!(parse_first_ip("\n\n"), None);
}

#[test]
fn parse_first_ip_rejects_non_address_text() {
    assert_eq!(parse_first_ip("not an ip\n"), None);
    assert_eq!(parse_first_ip("100.999.0.1\n"), None);
}
