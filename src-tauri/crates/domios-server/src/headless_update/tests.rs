use super::*;

#[test]
fn headless_backend_requires_exact_systemd_unit_and_installed_binary() {
    let executable = Path::new("/opt/domios-releases/current/domios-server/domios-server");
    assert!(managed(
        "0::/system.slice/domios-server.service\n",
        executable,
        executable
    ));
    assert!(!managed(
        "0::/system.slice/other.service\n",
        executable,
        executable
    ));
    assert!(!managed(
        "0::/system.slice/not-domios-server.service\n",
        executable,
        executable
    ));
    assert!(!managed(
        "0::/user.slice/user-1000.slice/domios-server.service\n",
        executable,
        executable
    ));
    assert!(!managed(
        "0::/system.slice/domios-server.service\n",
        Path::new("/tmp/server"),
        executable
    ));
}
