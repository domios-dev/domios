use axum::{
    extract::{Query, State as AxumState},
    http::{header, HeaderValue, StatusCode},
};
use serde::Deserialize;

use crate::api::routes::AppState;

const PLUGIN_BRIDGE_JS: &str = r#"
(function(){var p={},s=0;
window.addEventListener('message',function(e){var m=e.data||{};
 if(m.__ctReply!=null&&p[m.__ctReply]){var d=p[m.__ctReply];delete p[m.__ctReply];
  m.error?d.reject(new Error(m.error)):d.resolve(m.result);}
 if(m.__ctTheme!=null){document.documentElement.style.cssText=m.__ctTheme;}});
window.ct={surface:__CT_SURFACE__,invoke:function(method,args){var id=++s;return new Promise(function(res,rej){
  p[id]={resolve:res,reject:rej};parent.postMessage({__ctInvoke:id,method:method,args:args||null},'*');});},
 close:function(){parent.postMessage({__ctClose:true},'*');}};
parent.postMessage({__ctReady:true},'*');
function rz(){if(!document.body)return;var h=document.body.scrollHeight;if(h)parent.postMessage({__ctResize:h},'*');}
function watch(){if(window.ResizeObserver){new ResizeObserver(rz).observe(document.body);}rz();}
document.readyState==='loading'?document.addEventListener('DOMContentLoaded',watch):watch();})();
"#;

#[derive(Default, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginUiQuery {
    pub surface: Option<String>,
    pub instance: Option<String>,
    pub tab: Option<String>,
    #[serde(skip_serializing, rename = "reload")]
    pub _reload: Option<u64>,
}

fn ui_mime(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

pub async fn plugin_ui(
    AxumState(_state): AxumState<AppState>,
    axum::extract::Path((id, path)): axum::extract::Path<(String, String)>,
    Query(q): Query<PluginUiQuery>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let dir = crate::view_host::plugin_ui_dir(&id)
        .ok_or((StatusCode::NOT_FOUND, format!("no ui for plugin '{id}'")))?;
    let rel = if path.is_empty() {
        "index.html"
    } else {
        path.as_str()
    };
    if rel.contains("..") {
        return Err((StatusCode::BAD_REQUEST, "path traversal".into()));
    }
    let file = dir.join(rel);
    let bytes = tokio::fs::read(&file)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, "not found".to_string()))?;

    let mut resp = if rel.ends_with(".html") {
        html_response(&bytes, &q)
    } else {
        asset_response(rel, bytes)
    };
    let cache = if rel.ends_with(".html") {
        "no-store, must-revalidate"
    } else {
        "private, max-age=31536000, immutable"
    };
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    Ok(resp)
}

fn html_response(bytes: &[u8], context: &PluginUiQuery) -> axum::response::Response {
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let raw = String::from_utf8_lossy(bytes).replace("__CT_NONCE__", &nonce);
    let context = serde_json::to_string(context)
        .unwrap_or_else(|_| "null".into())
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
    let bridge_js = PLUGIN_BRIDGE_JS.replace("__CT_SURFACE__", &context);
    let bridge = format!("<script nonce=\"{nonce}\">{bridge_js}</script>");
    let html = match raw.find("<head>") {
        Some(i) => format!("{}{}{}", &raw[..i + 6], bridge, &raw[i + 6..]),
        None => format!("{bridge}{raw}"),
    };
    let csp = format!(
        "default-src 'none'; script-src 'nonce-{nonce}'; style-src 'nonce-{nonce}' 'unsafe-inline'; \
         img-src blob: data:; font-src blob: data:; connect-src 'none'; base-uri 'none'; form-action 'none'"
    );
    let mut response = axum::response::Response::new(axum::body::Body::from(html));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    if let Ok(value) = HeaderValue::from_str(&csp) {
        response
            .headers_mut()
            .insert(header::CONTENT_SECURITY_POLICY, value);
    }
    response
}

fn asset_response(rel: &str, bytes: Vec<u8>) -> axum::response::Response {
    let mut response = axum::response::Response::new(axum::body::Body::from(bytes));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(ui_mime(rel)));
    response
}

#[cfg(test)]
mod tests;
