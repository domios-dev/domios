use axum::{
    extract::{Query, State as AxumState},
    http::{header, HeaderValue, StatusCode},
};
use serde::Deserialize;

use crate::routes::AppState;

const PLUGIN_BRIDGE_JS: &str = r#"
(function(){var p={},s=0,subscribers=new Set();
window.addEventListener('message',function(e){var m=e.data||{};
 if(m.__ctReply!=null&&p[m.__ctReply]){var d=p[m.__ctReply];delete p[m.__ctReply];
  m.error?d.reject(new Error(m.error)):d.resolve(m.result);}
 if(e.source===parent&&m.__ctSurfaceStateChanged===true){subscribers.forEach(function(fn){try{fn();}catch(_){}});}
 if(m.__ctTheme!=null){document.documentElement.style.cssText=m.__ctTheme;}});
window.ct={surface:__CT_SURFACE__,invoke:function(method,args){var id=++s;return new Promise(function(res,rej){
  p[id]={resolve:res,reject:rej};parent.postMessage({__ctInvoke:id,method:method,args:args||null},'*');});},
 media:function(id){var key=++s;return new Promise(function(res,rej){p[key]={resolve:res,reject:rej};parent.postMessage({__ctMedia:key,id:id},'*');});},
 close:function(){parent.postMessage({__ctClose:true},'*');}};
window.ct.host=(function(){function ask(action,args){var id=++s;return new Promise(function(res){
  p[id]={resolve:res,reject:function(e){res({ok:false,code:'unavailable',reason:String(e&&e.message||e)});}};
  parent.postMessage({__ctHost:id,action:action,args:args},'*');});}
 var known,waiting=[];window.addEventListener('message',function(e){var m=e.data||{};
  if(e.source===parent&&m.__ctHostSupport!=null&&!known){known=m.__ctHostSupport;waiting.splice(0).forEach(function(f){f(known);});}});
 return{availability:function(){return known?Promise.resolve(known):new Promise(function(res){waiting.push(res);});},
  copyText:function(text){return ask('copyText',{text:text});},
  saveAs:function(handle,suggestedName){return ask('saveAs',{handle:handle,suggestedName:suggestedName});},
  reveal:function(handle){return ask('reveal',{handle:handle});}};})();
window.ct.onSurfaceStateChanged=function(fn){if(typeof fn!=="function")throw new Error("callback required");subscribers.add(fn);return function(){subscribers.delete(fn);};};
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
    AxumState(state): AxumState<AppState>,
    caller: Option<axum::Extension<crate::auth::CallerIdentity>>,
    axum::extract::Path((id, path)): axum::extract::Path<(String, String)>,
    Query(q): Query<PluginUiQuery>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let rel = if path.is_empty() {
        "index.html"
    } else {
        path.as_str()
    };
    if rel.contains("..") {
        return Err((StatusCode::BAD_REQUEST, "path traversal".into()));
    }
    let view = if rel.ends_with(".html") {
        if !matches!(
            caller.as_ref().map(|value| &value.0),
            Some(crate::auth::CallerIdentity::Local)
        ) {
            return Err((
                StatusCode::FORBIDDEN,
                "plugin view requires local owner".into(),
            ));
        }
        let registry = state
            .svc
            .ext::<codeterm_plugin::view_host::ViewRegistryHandle>()
            .clone();
        let provider = registry
            .get(&id)
            .ok_or((StatusCode::NOT_FOUND, "plugin view unavailable".into()))?;
        let context = state
            .svc
            .plugin_view_context(
                &id,
                q.surface.as_deref(),
                q.instance.as_deref(),
                q.tab.as_deref(),
            )
            .map_err(|error| (StatusCode::FORBIDDEN, error))?;
        Some((provider, registry, context))
    } else {
        None
    };
    let relative = rel.to_owned();
    let bytes = tokio::task::spawn_blocking(move || {
        codeterm_plugin::bundle_access::read_plugin_files(&id, || {
            let dir = codeterm_service_core::view_host::plugin_ui_dir(&id)
                .ok_or((StatusCode::NOT_FOUND, format!("no ui for plugin '{id}'")))?;
            std::fs::read(dir.join(relative))
                .map_err(|_| (StatusCode::NOT_FOUND, "not found".to_string()))
        })
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;

    let mut response = plugin_ui_response(rel, bytes, &q);
    if let Some((provider, registry, context)) = view {
        state
            .svc
            .validate_plugin_view_context(&context)
            .map_err(|error| (StatusCode::FORBIDDEN, error))?;
        let registration = codeterm_plugin::media_delivery::create_frame(
            &provider, &registry, context,
        )
        .map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "plugin view registration unavailable".into(),
            )
        })?;
        response.headers_mut().insert(
            "x-domios-plugin-view-frame",
            HeaderValue::from_str(&registration.view_lease).expect("host token is ASCII"),
        );
        if provider.media_delivery().is_some() {
            response.headers_mut().insert(
                "x-domios-plugin-media-frame",
                HeaderValue::from_str(&registration.media_frame).expect("host token is ASCII"),
            );
        }
    }
    Ok(response)
}

fn plugin_ui_response(
    rel: &str,
    bytes: Vec<u8>,
    context: &PluginUiQuery,
) -> axum::response::Response {
    let mut resp = if rel.ends_with(".html") {
        html_response(&bytes, context)
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
    resp
}

fn plugin_frame_csp(nonce: &str) -> String {
    // A style nonce suppresses unsafe-inline, blocking attributes and Git's unnonced runtime styles.
    format!(
        "default-src 'none'; script-src 'nonce-{nonce}'; style-src 'unsafe-inline'; \
         img-src blob: data:; font-src blob: data:; media-src 'none'; \
         connect-src 'none'; base-uri 'none'; form-action 'none'"
    )
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
    let csp = plugin_frame_csp(&nonce);
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
