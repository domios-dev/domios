const STATUS_SIGNATURE: &str = "pub async fn status(";
const CAPABILITY_SRC: &str = include_str!("../../../codeterm-secrets/src/capability.rs");
const LIB_SRC: &str = include_str!("../../../codeterm-secrets/src/lib.rs");
const JS_BACKEND_SRC: &str = include_str!("../../../codeterm-secrets/src/js_backend.rs");
const LEGACY_SRC: &str = include_str!("../../../codeterm-secrets/src/legacy.rs");

fn decision_half() -> &'static str {
    CAPABILITY_SRC
        .split("#[cfg(test)]")
        .next()
        .expect("capability.rs has a non-test half")
}

/// One top-level `fn`, sliced by lines so the check never depends on the
/// checkout's line endings - `str::lines` drops a trailing carriage return.
fn top_level_fn(src: &str, signature: &str) -> String {
    let mut lines = src.lines().skip_while(|l| !l.contains(signature));
    let mut body = vec![lines.next().expect("source declares the function")];
    for line in lines {
        body.push(line);
        if line == "}" {
            return body.join("\n");
        }
    }
    panic!("{signature} is not terminated by a closing brace at column zero");
}

#[test]
fn no_status_path_module_reads_the_retired_credential_drop() {
    for src in [
        include_str!("../api/secrets_routes.rs"),
        decision_half(),
        LIB_SRC,
        JS_BACKEND_SRC,
    ] {
        assert!(
            !src.contains("bitwarden-master.env"),
            "no status-path module may read the retired credential drop"
        );
    }
    assert!(
        LEGACY_SRC.contains("remove_file"),
        "the one module naming the legacy drop must be the one that erases it"
    );
}

#[test]
fn status_handler_names_no_provider() {
    let status_handler = top_level_fn(include_str!("../api/secrets_routes.rs"), STATUS_SIGNATURE);
    assert!(
        status_handler.contains("master_password_saved"),
        "extraction missed the real status handler"
    );
    assert!(
        !status_handler.contains("bitwarden"),
        "the status handler must not branch on a provider id"
    );
    assert!(
        !decision_half().contains("bitwarden"),
        "the auto-unlock decision must not name a provider"
    );
}

#[test]
fn the_status_extractor_catches_a_provider_named_handler() {
    let named = concat!(
        "pub async fn status(\n",
        "    state: State,\n",
        ") -> Json {\n",
        "    let saved = store.id() == \"bitwarden\";\n",
        "}\n",
        "pub fn unrelated() -> u8 { 0 }\n",
    );
    let crlf = named.replace('\n', "\r\n");
    assert!(
        crlf.contains("\r\n"),
        "the CRLF fixture must really be CRLF"
    );
    for src in [named, crlf.as_str()] {
        let body = top_level_fn(src, STATUS_SIGNATURE);
        assert!(body.contains("bitwarden"), "control must detect the id");
        assert!(
            !body.contains("unrelated"),
            "extraction must stop at the handler's closing brace"
        );
    }
}

#[test]
fn plugin_bucket_name_matches_the_host_api_convention() {
    let src = include_str!("../service/plugin_host.rs");
    assert!(
        src.contains("crate::secrets::plugin_secret_bucket"),
        "plugin_host must resolve its bucket through the shared helper"
    );
}
