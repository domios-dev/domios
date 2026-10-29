use axum::{
    extract::{Query, State as AxumState},
    http::{header, HeaderValue, StatusCode},
};
use serde::Deserialize;

use crate::routes::AppState;

const PLUGIN_BRIDGE_JS: &str = r#"
(function(){var p={},s=0,subscribers=new Set(),channel=new MessageChannel(),port=channel.port2,known,waiting=[];
var send=port.postMessage.bind(port);
port.onmessage=function(e){var m=e.data||{};
 if(m.__ctChallenge!=null){send({__ctProof:m.__ctChallenge});return;}
 if(m.__ctReply!=null&&p[m.__ctReply]){var d=p[m.__ctReply];delete p[m.__ctReply];
  m.error?d.reject(new Error(m.error)):d.resolve(m.result);}
 if(m.__ctSurfaceStateChanged===true){subscribers.forEach(function(fn){try{fn();}catch(_){}});}
 if(m.__ctHostSupport!=null&&!known){known=m.__ctHostSupport;waiting.splice(0).forEach(function(f){f(known);});}
 if(m.__ctTheme!=null){document.documentElement.style.cssText=m.__ctTheme;}
 if(m.__ctActive!=null||m.__ctKpiChanged!=null){window.dispatchEvent(new MessageEvent('message',{data:m,source:parent}));}
};port.start();
window.ct={surface:__CT_SURFACE__,invoke:function(method,args){var id=++s;return new Promise(function(res,rej){
 p[id]={resolve:res,reject:rej};send({__ctInvoke:id,method:method,args:args||null});});},
 media:function(id){var key=++s;return new Promise(function(res,rej){p[key]={resolve:res,reject:rej};send({__ctMedia:key,id:id});});},
 close:function(){send({__ctClose:true});}};
window.ct.host=(function(){function ask(action,args){var id=++s;return new Promise(function(res){
 p[id]={resolve:res,reject:function(e){res({ok:false,code:'unavailable',reason:String(e&&e.message||e)});}};
 send({__ctHost:id,action:action,args:args});});}
 return{availability:function(){return known?Promise.resolve(known):new Promise(function(res){waiting.push(res);});},
 copyText:function(text){return ask('copyText',{text:text});},
 saveAs:function(handle,suggestedName){return ask('saveAs',{handle:handle,suggestedName:suggestedName});},
 reveal:function(handle){return ask('reveal',{handle:handle});}};})();
window.ct.onSurfaceStateChanged=function(fn){if(typeof fn!=="function")throw new Error("callback required");subscribers.add(fn);return function(){subscribers.delete(fn);};};
window.ct.__reportUiState=function(state){send({__ctUiState:state});};
window.dispatchEvent(new Event('ct-bridge-ready'));
parent.postMessage({__ctReady:__CT_DOCUMENT__},'*',[channel.port1]);
function rz(){if(!document.body)return;var h=document.body.scrollHeight;if(h)send({__ctResize:h});}
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

    let mut response;
    if let Some((provider, registry, context)) = view {
        state
            .svc
            .validate_plugin_view_context(&context)
            .map_err(|error| (StatusCode::FORBIDDEN, error))?;
        let provider_registration = registry
            .registration_id(provider.id())
            .ok_or((StatusCode::NOT_FOUND, "view unavailable".into()))?;
        let registration = codeterm_plugin::media_delivery::create_frame(
            &provider, &registry, context,
        )
        .map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "plugin view registration unavailable".into(),
            )
        })?;
        response = plugin_ui_response(rel, bytes, &q, &registration.document);
        response.headers_mut().insert(
            "x-domios-plugin-view-document",
            HeaderValue::from_str(&registration.document).expect("host document is ASCII"),
        );
        if registry.registration_id(provider.id()).as_deref()
            != Some(provider_registration.as_str())
        {
            codeterm_plugin::media_delivery::revoke_view_frame(
                provider.id(),
                &registration.view_lease,
            );
            return Err((StatusCode::FORBIDDEN, "view provider changed".into()));
        }
        let context = &registration.context;
        let scope = crate::plugin_media_routes::ViewRegistrationScope {
            surface: context.surface.clone(),
            instance: context.instance.clone(),
            tab: context.tab.clone(),
            owner: context.owner.clone(),
            workspace: context.workspace.clone(),
            team: context.team.clone(),
            provider_registration,
        };
        let snapshot =
            serde_json::json!({"registration": registration.media_frame, "scope": scope});
        response.headers_mut().insert(
            "x-domios-plugin-view-registration",
            HeaderValue::from_str(&snapshot.to_string()).map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "invalid view scope header".into(),
                )
            })?,
        );
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
    } else {
        response = plugin_ui_response(rel, bytes, &q, "");
    }
    Ok(response)
}

fn plugin_ui_response(
    rel: &str,
    bytes: Vec<u8>,
    context: &PluginUiQuery,
    document: &str,
) -> axum::response::Response {
    let mut resp = if rel.ends_with(".html") {
        html_response(&bytes, context, document)
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

fn html_response(
    bytes: &[u8],
    context: &PluginUiQuery,
    document: &str,
) -> axum::response::Response {
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let raw = String::from_utf8_lossy(bytes).replace("__CT_NONCE__", &nonce);
    let context = serde_json::to_string(context)
        .unwrap_or_else(|_| "null".into())
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
    let bridge_js = PLUGIN_BRIDGE_JS
        .replace("__CT_SURFACE__", &context)
        .replace(
            "__CT_DOCUMENT__",
            &serde_json::to_string(document).expect("document string"),
        );
    let bridge = format!("<script nonce=\"{nonce}\">{bridge_js}</script>");
    let html = format!("<!doctype html>{bridge}{raw}");
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
