use super::*;
use serde_json::json;

#[test]
fn macos_self_detection_uses_localapi_then_bundle_then_interfaces_without_path() {
    let _native_entry: fn() -> TransportStatus = macos::detect;
    for success in [Probe::LocalApi, Probe::BundledCli, Probe::Interfaces] {
        let mut visited = Vec::new();
        let (status, _) = detect_with(true, |probe, failures| {
            visited.push(probe);
            if probe == success {
                address_status(vec!["100.96.252.97".parse().unwrap()])
            } else {
                failures.push(Failure::io(
                    "probe",
                    std::io::Error::from(std::io::ErrorKind::NotFound),
                ));
                None
            }
        });
        let expected = match success {
            Probe::LocalApi => vec![Probe::LocalApi],
            Probe::BundledCli => vec![Probe::LocalApi, Probe::BundledCli],
            Probe::Interfaces => vec![Probe::LocalApi, Probe::BundledCli, Probe::Interfaces],
        };
        assert_eq!(visited, expected);
        assert_eq!(status.state, TransportState::Connected);
        assert_eq!(
            status.addresses,
            vec!["100.96.252.97".parse::<IpAddr>().unwrap()]
        );
    }
}

#[test]
fn macos_self_detection_falls_back_after_running_without_addresses_but_respects_stopped() {
    let mut visits = 0;
    let (status, _) = detect_with(true, |_, _| {
        visits += 1;
        match visits {
            1 => Some(json!({"BackendState": "Running", "Self": {"TailscaleIPs": []}})),
            _ => address_status(vec!["fd7a:115c:a1e0::7".parse().unwrap()]),
        }
    });
    assert_eq!(visits, 2);
    assert_eq!(status.state, TransportState::Connected);
    assert_eq!(status.addresses.len(), 1);
    let mut visits = 0;
    let (status, _) = detect_with(true, |_, _| {
        visits += 1;
        Some(json!({"BackendState": "Stopped"}))
    });
    assert_eq!(visits, 1);
    assert_eq!(status.state, TransportState::NotRunning);
}

#[test]
fn macos_self_detection_permission_is_distinct_and_success_overrides_failed_probes() {
    let denied = Failure::io(
        "Group Containers",
        std::io::ErrorKind::PermissionDenied.into(),
    );
    assert_eq!(denied.kind, FailureKind::Permission);
    assert!(denied.detail.contains("Group Containers"));
    let missing = Failure::io("ipnport", std::io::ErrorKind::NotFound.into());
    let unavailable = Failure::io("localapi", std::io::ErrorKind::ConnectionRefused.into());
    assert_eq!(
        unavailable_state(true, &[missing.clone(), unavailable]),
        TransportState::NotRunning
    );
    assert_eq!(
        unavailable_state(false, &[missing]),
        TransportState::NotInstalled
    );
    assert_eq!(
        unavailable_state(true, &[denied.clone()]),
        TransportState::PermissionNeeded
    );
    assert_eq!(
        unavailable_state(true, &[Failure::http("localapi", 500)]),
        TransportState::Unknown
    );
    for code in [401, 403] {
        let failure = Failure::http("localapi", code);
        assert_eq!(failure.kind, FailureKind::Permission);
        assert_eq!(failure.detail, format!("localapi: HTTP {code}"));
    }
    let (status, failures) = detect_with(true, |probe, failures| {
        if probe == Probe::LocalApi {
            failures.push(denied.clone());
            None
        } else {
            address_status(vec!["100.96.252.97".parse().unwrap()])
        }
    });
    assert_eq!(status.state, TransportState::Connected);
    assert_eq!(failures, vec![denied]);
}

#[test]
fn macos_self_detection_quit_app_and_denied_container_have_honest_states() {
    for (denied, expected) in [
        (false, TransportState::NotRunning),
        (true, TransportState::PermissionNeeded),
    ] {
        let (status, failures) = detect_with(true, |probe, failures| {
            match probe {
                Probe::LocalApi => {
                    failures.push(Failure::io(
                        "Group Containers",
                        if denied {
                            std::io::ErrorKind::PermissionDenied
                        } else {
                            std::io::ErrorKind::ConnectionRefused
                        }
                        .into(),
                    ));
                    failures.push(Failure::io(
                        "/Library/Tailscale/ipnport",
                        std::io::ErrorKind::NotFound.into(),
                    ));
                }
                Probe::BundledCli => {
                    failures.push(Failure::io(
                        "Tailscale ip -4",
                        std::io::Error::other(codeterm_util::process_util::CaptureExit {
                            code: Some(1),
                            stderr: "Tailscale is not running".into(),
                        }),
                    ));
                    failures.push(Failure {
                        kind: FailureKind::Unavailable,
                        detail: "Tailscale ip -6: no tailnet address".into(),
                    });
                }
                Probe::Interfaces => failures.push(Failure {
                    kind: FailureKind::Unavailable,
                    detail: "no tailnet address".into(),
                }),
            }
            None
        });
        assert_eq!(status.state, expected);
        assert_eq!(failures.len(), 5);
    }
}

#[test]
fn macos_self_detection_filters_interface_name_and_exact_cgnat_ula_prefixes() {
    for ip in [
        "100.64.0.0",
        "100.96.252.97",
        "100.127.255.255",
        "fd7a:115c:a1e0::1",
        "fd7a:115c:a1e0:ffff::1",
    ] {
        for name in ["utun7", "tailscale0", "Tailscale"] {
            assert!(interface_address(name, ip.parse().unwrap()), "{name} {ip}");
        }
        for name in ["en0", "lo0", "utap0"] {
            assert!(!interface_address(name, ip.parse().unwrap()), "{name} {ip}");
        }
    }
    for ip in [
        "100.63.255.255",
        "100.128.0.0",
        "192.168.1.1",
        "127.0.0.1",
        "fd7a:115c:a1e1::1",
        "fd00::1",
        "::1",
        "fe80::1",
    ] {
        assert!(!interface_address("utun7", ip.parse().unwrap()), "{ip}");
    }
}
