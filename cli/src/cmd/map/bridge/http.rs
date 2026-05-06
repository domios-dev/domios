use crate::cmd::map::lang::Lang;
use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpHandler {
    pub method: String,
    pub path: String,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpCall {
    pub method: String,
    pub path: String,
    pub line: u32,
}

pub fn extract_handlers(lang: Lang, source: &str) -> Vec<HttpHandler> {
    match lang {
        Lang::Python => python_handlers(source),
        Lang::JavaScript | Lang::TypeScript | Lang::Tsx => js_handlers(source),
        Lang::Rust => rust_handlers(source),
        _ => Vec::new(),
    }
}

pub fn extract_calls(lang: Lang, source: &str) -> Vec<HttpCall> {
    match lang {
        Lang::JavaScript | Lang::TypeScript | Lang::Tsx => js_calls(source),
        Lang::Python => python_calls(source),
        Lang::Rust => rust_calls(source),
        _ => Vec::new(),
    }
}

static PY_HANDLER: OnceLock<Regex> = OnceLock::new();
fn py_handler_re() -> &'static Regex {
    PY_HANDLER.get_or_init(|| {
        // FastAPI/Flask: @app.get("/path"), @router.post("/path"), @app.route("/p", methods=["GET"])
        Regex::new(
            r#"(?m)@\s*[A-Za-z_][A-Za-z0-9_]*\s*\.\s*(get|post|put|delete|patch|route)\s*\(\s*['"]([^'"]+)['"]"#,
        )
        .expect("py handler regex")
    })
}

fn python_handlers(source: &str) -> Vec<HttpHandler> {
    let mut out = Vec::new();
    let re = py_handler_re();
    for cap in re.captures_iter(source) {
        let raw_method = cap.get(1).expect("method").as_str();
        let path = cap.get(2).expect("path").as_str();
        let method = if raw_method.eq_ignore_ascii_case("route") {
            "*".into()
        } else {
            raw_method.to_uppercase()
        };
        let line = byte_to_line(source, cap.get(0).unwrap().start());
        out.push(HttpHandler { method, path: path.to_string(), line });
    }
    out
}

static PY_CALL: OnceLock<Regex> = OnceLock::new();
fn py_call_re() -> &'static Regex {
    PY_CALL.get_or_init(|| {
        // requests/httpx: requests.get("https://…/path"), client.post("/path")
        Regex::new(
            r#"\b(?:requests|client|httpx|session)\s*\.\s*(get|post|put|delete|patch)\s*\(\s*['"]([^'"]+)['"]"#,
        )
        .expect("py call regex")
    })
}

fn python_calls(source: &str) -> Vec<HttpCall> {
    let mut out = Vec::new();
    for cap in py_call_re().captures_iter(source) {
        let method = cap.get(1).unwrap().as_str().to_uppercase();
        let path = strip_origin(cap.get(2).unwrap().as_str());
        let line = byte_to_line(source, cap.get(0).unwrap().start());
        out.push(HttpCall { method, path, line });
    }
    out
}

static JS_HANDLER: OnceLock<Regex> = OnceLock::new();
fn js_handler_re() -> &'static Regex {
    JS_HANDLER.get_or_init(|| {
        // Express/Hono: app.get('/x', …), router.post('/x', …)
        Regex::new(
            r#"\b(?:app|router)\s*\.\s*(get|post|put|delete|patch|use|route)\s*\(\s*['"`]([^'"`]+)['"`]"#,
        )
        .expect("js handler regex")
    })
}

fn js_handlers(source: &str) -> Vec<HttpHandler> {
    let mut out = Vec::new();
    for cap in js_handler_re().captures_iter(source) {
        let raw_method = cap.get(1).unwrap().as_str();
        if raw_method.eq_ignore_ascii_case("use") {
            continue;
        }
        let method = if raw_method.eq_ignore_ascii_case("route") {
            "*".into()
        } else {
            raw_method.to_uppercase()
        };
        let path = cap.get(2).unwrap().as_str().to_string();
        let line = byte_to_line(source, cap.get(0).unwrap().start());
        out.push(HttpHandler { method, path, line });
    }
    out
}

static JS_FETCH: OnceLock<Regex> = OnceLock::new();
fn js_fetch_re() -> &'static Regex {
    JS_FETCH.get_or_init(|| {
        // fetch('/x'), fetch(`/x/${id}`), axios.get('/x')
        Regex::new(
            r#"\b(?:fetch|axios\.\s*(?:get|post|put|delete|patch)|axios)\s*\(\s*['"`]([^'"`]+)['"`]"#,
        )
        .expect("js fetch regex")
    })
}

static JS_FETCH_METHOD: OnceLock<Regex> = OnceLock::new();
fn js_fetch_method_re() -> &'static Regex {
    JS_FETCH_METHOD.get_or_init(|| Regex::new(r#"axios\s*\.\s*(get|post|put|delete|patch)"#).expect("axios method"))
}

fn js_calls(source: &str) -> Vec<HttpCall> {
    let mut out = Vec::new();
    for cap in js_fetch_re().captures_iter(source) {
        let m = cap.get(0).unwrap().as_str();
        let method = js_fetch_method_re()
            .captures(m)
            .and_then(|c| c.get(1))
            .map(|x| x.as_str().to_uppercase())
            .unwrap_or_else(|| "GET".into());
        let raw = cap.get(1).unwrap().as_str();
        let path = strip_origin(raw);
        if path.starts_with('/') || path.starts_with("http") {
            let line = byte_to_line(source, cap.get(0).unwrap().start());
            out.push(HttpCall { method, path, line });
        }
    }
    out
}

static RUST_HANDLER: OnceLock<Regex> = OnceLock::new();
fn rust_handler_re() -> &'static Regex {
    RUST_HANDLER.get_or_init(|| {
        // Axum: Router::new().route("/x", get(handler)) ; Rocket #[get("/x")]
        Regex::new(
            r#"\.route\s*\(\s*"([^"]+)"\s*,\s*(get|post|put|delete|patch)\s*\("#,
        )
        .expect("rust handler regex")
    })
}

static RUST_HANDLER_ATTR: OnceLock<Regex> = OnceLock::new();
fn rust_handler_attr_re() -> &'static Regex {
    RUST_HANDLER_ATTR.get_or_init(|| {
        Regex::new(r#"#\[\s*(get|post|put|delete|patch)\s*\(\s*"([^"]+)""#).expect("rust attr regex")
    })
}

fn rust_handlers(source: &str) -> Vec<HttpHandler> {
    let mut out = Vec::new();
    for cap in rust_handler_re().captures_iter(source) {
        let path = cap.get(1).unwrap().as_str().to_string();
        let method = cap.get(2).unwrap().as_str().to_uppercase();
        let line = byte_to_line(source, cap.get(0).unwrap().start());
        out.push(HttpHandler { method, path, line });
    }
    for cap in rust_handler_attr_re().captures_iter(source) {
        let method = cap.get(1).unwrap().as_str().to_uppercase();
        let path = cap.get(2).unwrap().as_str().to_string();
        let line = byte_to_line(source, cap.get(0).unwrap().start());
        out.push(HttpHandler { method, path, line });
    }
    out
}

static RUST_CALL: OnceLock<Regex> = OnceLock::new();
fn rust_call_re() -> &'static Regex {
    RUST_CALL.get_or_init(|| {
        // reqwest::get("…"), client.get("…").send()
        Regex::new(
            r#"\b(?:reqwest|client|http)\s*\.\s*(get|post|put|delete|patch)\s*\(\s*"([^"]+)""#,
        )
        .expect("rust call regex")
    })
}

fn rust_calls(source: &str) -> Vec<HttpCall> {
    let mut out = Vec::new();
    for cap in rust_call_re().captures_iter(source) {
        let method = cap.get(1).unwrap().as_str().to_uppercase();
        let path = strip_origin(cap.get(2).unwrap().as_str());
        let line = byte_to_line(source, cap.get(0).unwrap().start());
        out.push(HttpCall { method, path, line });
    }
    out
}

fn strip_origin(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix("https://") {
        slash_path(rest)
    } else if let Some(rest) = raw.strip_prefix("http://") {
        slash_path(rest)
    } else {
        raw.to_string()
    }
}

fn slash_path(host_and_path: &str) -> String {
    match host_and_path.find('/') {
        Some(i) => host_and_path[i..].to_string(),
        None => "/".to_string(),
    }
}

fn byte_to_line(source: &str, byte_offset: usize) -> u32 {
    let mut line = 0u32;
    for (i, b) in source.bytes().enumerate() {
        if i >= byte_offset {
            return line;
        }
        if b == b'\n' {
            line += 1;
        }
    }
    line
}

pub fn match_path(handler: &str, call: &str) -> bool {
    if handler == call {
        return true;
    }
    let h_parts: Vec<&str> = handler.split('/').collect();
    let c_parts: Vec<&str> = call.split('/').collect();
    if h_parts.len() != c_parts.len() {
        return false;
    }
    for (h, c) in h_parts.iter().zip(c_parts.iter()) {
        if h.starts_with('{') && h.ends_with('}') {
            continue;
        }
        if h.starts_with('<') && h.ends_with('>') {
            continue;
        }
        if h.starts_with(':') {
            continue;
        }
        if h != c && c.starts_with("${") {
            continue;
        }
        if h != c {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fastapi_handler() {
        let src = r#"
@app.get("/api/users")
def list_users(): pass

@router.post('/api/users/{id}')
def update_user(id): pass
"#;
        let h = python_handlers(src);
        assert!(h.iter().any(|x| x.method == "GET" && x.path == "/api/users"));
        assert!(h.iter().any(|x| x.method == "POST" && x.path == "/api/users/{id}"));
    }

    #[test]
    fn express_handler() {
        let src = r#"
app.get('/api/users', listUsers);
router.post(`/api/users/:id`, updateUser);
"#;
        let h = js_handlers(src);
        assert!(h.iter().any(|x| x.method == "GET" && x.path == "/api/users"));
        assert!(h.iter().any(|x| x.method == "POST" && x.path == "/api/users/:id"));
    }

    #[test]
    fn axum_handler_and_attr() {
        let src = r#"
let app = Router::new()
    .route("/api/users", get(list_users))
    .route("/api/users/:id", post(update_user));

#[get("/legacy/health")]
fn health() {}
"#;
        let h = rust_handlers(src);
        assert!(h.iter().any(|x| x.method == "GET" && x.path == "/api/users"));
        assert!(h.iter().any(|x| x.method == "POST" && x.path == "/api/users/:id"));
        assert!(h.iter().any(|x| x.method == "GET" && x.path == "/legacy/health"));
    }

    #[test]
    fn fetch_call_strips_origin() {
        let src = r#"
const r = fetch("https://api.example.com/v1/items");
const r2 = await axios.post('/api/users/123', body);
"#;
        let c = js_calls(src);
        assert!(c.iter().any(|x| x.method == "GET" && x.path == "/v1/items"));
        assert!(c.iter().any(|x| x.method == "POST" && x.path == "/api/users/123"));
    }

    #[test]
    fn match_with_param() {
        assert!(match_path("/api/users/{id}", "/api/users/42"));
        assert!(match_path("/api/users/:id", "/api/users/abc"));
        assert!(!match_path("/api/users/{id}", "/api/posts/42"));
        assert!(!match_path("/api/users", "/api/users/42"));
    }
}
