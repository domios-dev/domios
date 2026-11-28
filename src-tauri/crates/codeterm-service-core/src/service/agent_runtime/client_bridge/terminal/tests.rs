use super::{environment, Capture, Env, TRANSCRIPT_BYTES};
use std::collections::HashMap;

#[test]
fn terminal_transcript_keeps_head_tail_and_exact_omitted_byte_count() {
    let mut capture = Capture::default();
    let output = format!("k6/{}!/p3", "🌍".repeat(9_000));
    for chunk in output.as_bytes().chunks(97) {
        capture.append(chunk, 20_000);
    }
    let transcript = capture.transcript(20_000);
    let parts: Vec<_> = transcript.split('\n').collect();
    assert_eq!(parts.len(), 3);
    assert!(output.starts_with(parts[0]));
    assert!(output.ends_with(parts[2]));
    assert_eq!(
        parts[1],
        format!(
            "[{} bytes omitted]",
            output.len() - parts[0].len() - parts[2].len()
        )
    );
    assert!(transcript.len() <= TRANSCRIPT_BYTES);
}

#[test]
fn terminal_environment_strips_inherited_credentials() {
    let base = HashMap::from([
        ("PATH".into(), "/safe/bin".into()),
        ("OPENAI_API_KEY".into(), "secret".into()),
        ("CODETERM_API_TOKEN".into(), "secret".into()),
    ]);
    let env = environment(base, &[], &["PATH".into()]).unwrap();
    assert!(!env.contains_key("OPENAI_API_KEY"));
    assert!(!env.contains_key("CODETERM_API_TOKEN"));
    assert!(!env.contains_key("PATH"));
}

#[test]
fn terminal_environment_preserves_host_launch_identity_and_accepts_grok_changes() {
    let base = HashMap::from([
        (
            "CODETERM_LAUNCH_FILE".into(),
            "/private/own-launch.json".into(),
        ),
        ("CODETERM_TAB_ID".into(), "own-tab".into()),
        ("CODETERM_SESSION_BINDING_NONCE".into(), "own-nonce".into()),
        ("CODETERM_API_URL".into(), "http://127.0.0.1:17686".into()),
        ("CODETERM_API_TOKEN".into(), "own-token".into()),
    ]);
    let grok_keys = [
        "CLICOLOR",
        "CARGO_TERM_PROGRESS_WIDTH",
        "MAVEN_OPTS",
        "GRADLE_OPTS",
        "FORCE_COLOR",
        "COLORTERM",
        "TERM",
        "PIP_PROGRESS_BAR",
        "NPM_CONFIG_PROGRESS",
        "GROK_SESSION_ID",
        "CI",
        "CLICOLOR_FORCE",
        "CARGO_TERM_PROGRESS_WHEN",
    ];
    let protected_keys = [
        "CODETERM_LAUNCH_FILE",
        "CODETERM_TAB_ID",
        "CODETERM_SESSION_BINDING_NONCE",
        "CODETERM_API_URL",
        "codeterm_api_token",
        "CoDeTeRm",
        "Domios_Tab_Id",
        "DOMIOſ_API_TOKEN",
    ];
    let changes: Vec<_> = grok_keys
        .into_iter()
        .map(|name| Env {
            name: name.into(),
            value: "grok-fixture".into(),
        })
        .chain(protected_keys.into_iter().map(|name| Env {
            name: name.into(),
            value: "foreign".into(),
        }))
        .collect();
    let env = environment(base.clone(), &changes, &[]).unwrap();
    for key in grok_keys {
        assert_eq!(env[key], "grok-fixture");
    }
    for key in [
        "CODETERM_LAUNCH_FILE",
        "CODETERM_TAB_ID",
        "CODETERM_SESSION_BINDING_NONCE",
        "CODETERM_API_URL",
    ] {
        assert_eq!(env[key], base[key]);
    }
    for key in [
        "CODETERM_API_TOKEN",
        "codeterm_api_token",
        "CoDeTeRm",
        "Domios_Tab_Id",
        "DOMIOſ_API_TOKEN",
    ] {
        assert!(!env.contains_key(key));
    }
}

#[test]
fn terminal_environment_enforces_request_bounds() {
    for (name, value) in [
        ("", "value"),
        ("HAS=EQUALS", "value"),
        ("NAME\0", "value"),
        (&"K".repeat(4097), "value"),
        ("KEY", "value\0"),
        ("KEY", &"x".repeat(4097)),
        ("CODETERM_TAB_ID", &"x".repeat(4097)),
    ] {
        let entry = Env {
            name: name.into(),
            value: value.into(),
        };
        assert!(environment(HashMap::new(), &[entry], &[]).is_err());
    }
    let changes: Vec<_> = (0..32)
        .map(|index| Env {
            name: format!("FIXTURE_{index}"),
            value: "x".repeat(4096),
        })
        .collect();
    let env = environment(HashMap::new(), &changes, &[]).unwrap();
    assert_eq!(env["FIXTURE_31"].len(), 4096);
    let mut oversized = changes;
    oversized.push(Env {
        name: "CODETERM_TAB_ID".into(),
        value: "ignored".into(),
    });
    assert!(environment(HashMap::new(), &oversized, &[]).is_err());
}
