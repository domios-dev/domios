use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};

const MARK: &str = include_str!("../../../../icons/app-icon.svg");
const STYLE: &str = r#"
:root{color-scheme:light dark;font-family:system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;color:#182230;background:#f1f5f9}
*{box-sizing:border-box}body{margin:0;min-height:100vh;display:grid;place-items:center;padding:24px}
main{width:100%;max-width:440px;padding:40px 28px;text-align:center;background:#fff;border:1px solid #dbe3ed;border-radius:20px;box-shadow:0 16px 48px #18223012}
svg{width:64px;height:64px;border-radius:14px}h1{font-size:24px;letter-spacing:-.5px;margin:24px 0 12px}p{line-height:1.6;color:#526175;margin:12px 0}
a{display:inline-block;margin-top:16px;padding:12px 24px;border-radius:10px;background:#58a6ff;color:#0d1117;font-weight:600;text-decoration:none}a:focus-visible{outline:3px solid #2872c7;outline-offset:4px}small{display:block;margin-top:24px;color:#526175}
@media(prefers-color-scheme:dark){:root{color:#edf2f8;background:#0d1117}main{background:#161b22;border-color:#30363d;box-shadow:none}p,small{color:#a7b4c5}}
"#;

pub(super) fn response(accepted: bool, return_to_app: bool) -> String {
    let (status, title, copy) = if accepted {
        (
            "200 OK",
            "Return to Domios",
            "Your sign-in callback was received. Return to Domios to continue.",
        )
    } else {
        (
            "400 Bad Request",
            "Sign-in callback refused",
            "This sign-in link could not be accepted. Return to Domios and try signing in again.",
        )
    };
    let action = if accepted && return_to_app {
        r#"<a id="open-app" href="domios://focus">Open Domios</a>"#
    } else {
        ""
    };
    let open = if accepted && return_to_app {
        "window.location.replace(document.getElementById('open-app').href);"
    } else {
        ""
    };
    let script = format!("history.replaceState(null,'','/callback');{open}");
    let style_hash = STANDARD.encode(Sha256::digest(STYLE.as_bytes()));
    let script_hash = STANDARD.encode(Sha256::digest(script.as_bytes()));
    let body = format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{title} · Domios</title><style>{STYLE}</style></head><body><main>{MARK}<h1>{title}</h1><p>{copy}</p>{action}<small>You can close this tab.</small></main><script>{script}</script></body></html>"#
    );
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Security-Policy: default-src 'none'; style-src 'sha256-{style_hash}'; script-src 'sha256-{script_hash}'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}
