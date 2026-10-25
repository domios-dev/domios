#[path = "support/codeterm_bin.rs"]
mod codeterm_bin;
#[path = "support/test_support.rs"]
mod test_support;

#[cfg(any(unix, windows))]
#[test]
fn isolated_server_api_token_identify_and_shutdown() {
    let server = std::env::var_os("DOMIOS_CLI_TEST_SERVER")
        .filter(|path| !path.is_empty())
        .expect("Run through build.sh test-one cli isolated_server to prepare the real server");
    let output = std::process::Command::new("node")
        .arg(test_support::fixture_manifest_dir().join("tests/support/isolated_server.mjs"))
        .arg(server)
        .arg(codeterm_bin::codeterm_bin())
        .output()
        .expect("Run the isolated CLI/server fixture");
    assert!(
        output.status.success(),
        "isolated server integration failed ({}):\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    print!("{}", String::from_utf8_lossy(&output.stdout));
}
