//! `PluginVm` — one QuickJS context per plugin.

use std::path::Path;

use parking_lot::Mutex;
use rquickjs::{CatchResultExt, Context, Function, Runtime, Value};

#[derive(Debug)]
pub enum VmError {
    ReadFile { path: String, source: std::io::Error },
    RuntimeInit(String),
    Script(String),
    MissingFunction(String),
    InvalidJson(String, String),
    ArgSerialize(String, serde_json::Error),
}

impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmError::ReadFile { path, source } => {
                write!(f, "failed to read plugin file {path}: {source}")
            }
            VmError::RuntimeInit(msg) => write!(f, "failed to initialise JS runtime: {msg}"),
            VmError::Script(msg) => write!(f, "plugin script error: {msg}"),
            VmError::MissingFunction(name) => write!(f, "plugin missing exported function `{name}`"),
            VmError::InvalidJson(name, msg) => {
                write!(f, "plugin function `{name}` returned invalid JSON: {msg}")
            }
            VmError::ArgSerialize(name, err) => {
                write!(f, "argument serialization failed for `{name}`: {err}")
            }
        }
    }
}

impl std::error::Error for VmError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            VmError::ReadFile { source, .. } => Some(source),
            VmError::ArgSerialize(_, err) => Some(err),
            _ => None,
        }
    }
}

/// One plugin = one VM. Holds the QuickJS runtime + context behind a
/// mutex so cross-thread callers serialize cleanly. Cheap to clone via
/// `Arc<PluginVm>`.
pub struct PluginVm {
    inner: Mutex<Inner>,
    plugin_id: String,
}

struct Inner {
    // Order matters: context must be dropped before runtime.
    _ctx: Context,
    _rt: Runtime,
    // We re-create a fresh context per call would be expensive; instead
    // the bundle's exports live as globals (`__plugin__.detectFromTitle`,
    // ...) after the initial load. `call` reads them on demand.
    bundle_source: String,
}

impl PluginVm {
    /// Load a plugin bundle (`plugin.js`) into a fresh QuickJS context.
    ///
    /// The bundle is expected to be a self-contained ES module that
    /// assigns its default export's fields to a `globalThis.__plugin__`
    /// object. The loader wraps the bundle's source with a tiny
    /// preamble that does this, so plugin authors don't need to know
    /// about it.
    pub fn load(plugin_id: &str, plugin_js_path: &Path) -> Result<Self, VmError> {
        let source = std::fs::read_to_string(plugin_js_path).map_err(|e| VmError::ReadFile {
            path: plugin_js_path.display().to_string(),
            source: e,
        })?;
        Self::from_source(plugin_id, source)
    }

    /// Same as `load` but takes the source string directly — used in
    /// tests and by the in-repo bundled plugin loader.
    pub fn from_source(plugin_id: &str, bundle_source: String) -> Result<Self, VmError> {
        let rt = Runtime::new().map_err(|e| VmError::RuntimeInit(e.to_string()))?;
        let ctx = Context::full(&rt).map_err(|e| VmError::RuntimeInit(e.to_string()))?;

        // Bootstrap: install the plugin's exports onto globalThis as
        // `__plugin__`. esbuild output uses `module.exports.default = ...`
        // when configured for cjs; for esm it'd be `export default`. We
        // standardise on a tiny IIFE that the host wraps around the
        // bundle so plugin authors just `export default { ... }` and the
        // bootstrap turns that into a global.
        //
        // Author-side: their source contains `export default { detectFromTitle(...) {...} }`.
        // Build-side: esbuild bundles into a single file with the default
        // export. We then evaluate the bundle inside a wrapping function
        // that captures the default export and exposes it as
        // `globalThis.__plugin__`.
        let bootstrap = format!(
            r#"
            globalThis.__plugin__ = (function() {{
                const module = {{ exports: {{}} }};
                const exports = module.exports;
                {source}
                // ESM default export styles esbuild may emit:
                if (module.exports.default) return module.exports.default;
                if (module.exports.__esModule && module.exports.default) return module.exports.default;
                return module.exports;
            }})();
            "#,
            source = bundle_source
        );

        ctx.with(|ctx| -> Result<(), VmError> {
            ctx.eval::<(), _>(bootstrap.clone())
                .catch(&ctx)
                .map_err(|e| VmError::Script(format!("{e:?}")))?;
            Ok(())
        })?;

        Ok(Self {
            inner: Mutex::new(Inner {
                _ctx: ctx,
                _rt: rt,
                bundle_source,
            }),
            plugin_id: plugin_id.to_string(),
        })
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    /// Re-load the plugin from a fresh source string. Used by the
    /// hot-reload path when `plugin.js` changes on disk.
    pub fn reload(&self, new_source: String) -> Result<(), VmError> {
        let mut inner = self.inner.lock();
        let rt = Runtime::new().map_err(|e| VmError::RuntimeInit(e.to_string()))?;
        let ctx = Context::full(&rt).map_err(|e| VmError::RuntimeInit(e.to_string()))?;

        let bootstrap = format!(
            r#"
            globalThis.__plugin__ = (function() {{
                const module = {{ exports: {{}} }};
                const exports = module.exports;
                {source}
                if (module.exports.default) return module.exports.default;
                if (module.exports.__esModule && module.exports.default) return module.exports.default;
                return module.exports;
            }})();
            "#,
            source = new_source
        );
        ctx.with(|ctx| -> Result<(), VmError> {
            ctx.eval::<(), _>(bootstrap)
                .catch(&ctx)
                .map_err(|e| VmError::Script(format!("{e:?}")))?;
            Ok(())
        })?;
        inner._ctx = ctx;
        inner._rt = rt;
        inner.bundle_source = new_source;
        Ok(())
    }
}

/// Call a method on the plugin's default export.
///
/// Arguments are serialized as JSON strings on the Rust side, parsed
/// inside the JS VM, passed positionally to the plugin's function. The
/// return value is JSON-serialized inside the VM and deserialized on
/// the Rust side. Plugins that need to return `undefined` should return
/// `null` — `undefined` becomes `null` over the JSON bridge.
///
/// This lives as a free function (not a method) so callers can pass an
/// `Arc<PluginVm>` cheaply and not borrow the whole vm for the duration.
pub fn call_json<R: serde::de::DeserializeOwned>(
    vm: &PluginVm,
    method: &str,
    args: &[serde_json::Value],
) -> Result<R, VmError> {
    let inner = vm.inner.lock();
    let result_json: String = inner._ctx.with(|ctx| -> Result<String, VmError> {
        // Pull the function off __plugin__
        let plugin_obj: Value = ctx
            .globals()
            .get("__plugin__")
            .catch(&ctx)
            .map_err(|e| VmError::Script(format!("__plugin__ not present: {e:?}")))?;
        let plugin_obj = plugin_obj
            .as_object()
            .ok_or_else(|| VmError::Script("__plugin__ is not an object".into()))?;
        let func: Function = plugin_obj
            .get(method)
            .catch(&ctx)
            .map_err(|_| VmError::MissingFunction(method.to_string()))?;

        // Encode args as a single JSON string, parse inside the VM,
        // spread into the call. This avoids per-arg Value construction
        // and keeps the bridge boundary simple.
        let args_json = serde_json::to_string(args)
            .map_err(|e| VmError::ArgSerialize(method.to_string(), e))?;
        ctx.globals()
            .set("__plugin_args__", args_json)
            .catch(&ctx)
            .map_err(|e| VmError::Script(format!("setting args failed: {e:?}")))?;

        // Run a tiny helper that parses args + calls + serializes result.
        let helper = format!(
            r#"
            (function() {{
                const args = JSON.parse(globalThis.__plugin_args__);
                const fn = globalThis.__plugin__[{name:?}];
                if (typeof fn !== 'function') {{
                    throw new Error('not a function: ' + {name:?});
                }}
                const out = fn.apply(globalThis.__plugin__, args);
                // Promise returns aren't supported here — providers
                // declare sync methods only.
                if (out && typeof out.then === 'function') {{
                    throw new Error({name:?} + ' returned a Promise; plugin methods must be synchronous');
                }}
                return JSON.stringify(out === undefined ? null : out);
            }})()
            "#,
            name = method,
        );
        let result: String = ctx
            .eval(helper)
            .catch(&ctx)
            .map_err(|e| VmError::Script(format!("{method}: {e:?}")))?;
        Ok(result)
    })?;
    // Drop the lock before parsing.
    drop(inner);
    serde_json::from_str(&result_json)
        .map_err(|e| VmError::InvalidJson(method.to_string(), e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture(src: &str) -> PluginVm {
        PluginVm::from_source("test", src.to_string()).expect("load")
    }

    #[test]
    fn round_trip_string_arg_and_return() {
        let vm = fixture(
            r#"
            module.exports.default = {
                upper(s) { return s.toUpperCase(); }
            };
        "#,
        );
        let out: String = call_json(&vm, "upper", &[json!("hi")]).unwrap();
        assert_eq!(out, "HI");
    }

    #[test]
    fn round_trip_object_arg_and_return() {
        let vm = fixture(
            r#"
            module.exports.default = {
                meta() { return { id: "x", displayName: "X" }; }
            };
        "#,
        );
        let out: serde_json::Value = call_json(&vm, "meta", &[]).unwrap();
        assert_eq!(out["id"], "x");
        assert_eq!(out["displayName"], "X");
    }

    #[test]
    fn round_trip_array_return() {
        let vm = fixture(
            r#"
            module.exports.default = {
                things() { return [1, 2, 3]; }
            };
        "#,
        );
        let out: Vec<i32> = call_json(&vm, "things", &[]).unwrap();
        assert_eq!(out, vec![1, 2, 3]);
    }

    #[test]
    fn missing_function_errors() {
        let vm = fixture("module.exports.default = {};");
        let err = call_json::<()>(&vm, "nope", &[]).unwrap_err();
        match err {
            VmError::Script(_) | VmError::MissingFunction(_) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn plugin_throw_surfaces_as_script_error() {
        let vm = fixture(
            r#"
            module.exports.default = {
                kaboom() { throw new Error('nope'); }
            };
        "#,
        );
        let err = call_json::<()>(&vm, "kaboom", &[]).unwrap_err();
        assert!(matches!(err, VmError::Script(_)));
    }

    #[test]
    fn returning_undefined_becomes_null_in_rust() {
        let vm = fixture(
            r#"
            module.exports.default = {
                nothing() { /* implicit undefined */ }
            };
        "#,
        );
        let out: Option<i32> = call_json(&vm, "nothing", &[]).unwrap();
        assert_eq!(out, None);
    }

    #[test]
    fn promise_return_rejected() {
        let vm = fixture(
            r#"
            module.exports.default = {
                async slow() { return 1; }
            };
        "#,
        );
        let err = call_json::<i32>(&vm, "slow", &[]).unwrap_err();
        assert!(matches!(err, VmError::Script(_)));
    }

    #[test]
    fn boolean_return() {
        let vm = fixture(
            r#"
            module.exports.default = {
                yes() { return true; },
                no()  { return false; }
            };
        "#,
        );
        let yes: bool = call_json(&vm, "yes", &[]).unwrap();
        let no: bool = call_json(&vm, "no", &[]).unwrap();
        assert!(yes);
        assert!(!no);
    }

    #[test]
    fn esm_export_default_style() {
        // esbuild output for `export default { ... }` produces
        // `module.exports.default = ...` after the bootstrap interpret.
        let vm = fixture(
            r#"
            // Pretend this is esbuild output for `export default { ... }`
            Object.defineProperty(module.exports, "__esModule", { value: true });
            module.exports.default = {
                hello() { return "world"; }
            };
        "#,
        );
        let out: String = call_json(&vm, "hello", &[]).unwrap();
        assert_eq!(out, "world");
    }

    #[test]
    fn reload_swaps_implementation() {
        let vm = fixture(
            r#"
            module.exports.default = { v() { return 1; } };
        "#,
        );
        let v1: i32 = call_json(&vm, "v", &[]).unwrap();
        assert_eq!(v1, 1);

        vm.reload(
            r#"
            module.exports.default = { v() { return 2; } };
        "#
            .to_string(),
        )
        .unwrap();
        let v2: i32 = call_json(&vm, "v", &[]).unwrap();
        assert_eq!(v2, 2);
    }
}
