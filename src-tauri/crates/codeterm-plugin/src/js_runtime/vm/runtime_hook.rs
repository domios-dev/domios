use super::timing;
use super::*;
use crate::agent_runtime::{RuntimeFailure, RuntimeFailureCode, MAX_RUNTIME_VALUE_BYTES};

const RUNTIME_MEMORY_LIMIT: usize = 64 * 1024 * 1024;
// QuickJS re-arms cycle GC at 1.5x the live heap; above the limit it never fires again.
const RUNTIME_GC_THRESHOLD: usize = RUNTIME_MEMORY_LIMIT / 2;

const INVOKE: &str = r#"
(function () {
  const method = globalThis.__ct_runtime_method__;
  const fn = globalThis.__plugin__[method];
  if (typeof fn !== "function") throw new Error("missing runtime hook");
  const value = fn.apply(globalThis.__plugin__, JSON.parse(globalThis.__ct_runtime_args__));
  if (globalThis.__ct_runtime_denied_binding__ !== undefined)
    throw new Error("runtime hook host binding unavailable: " + globalThis.__ct_runtime_denied_binding__);
  const result = JSON.stringify(value, function (_key, entry) {
    if (entry === undefined || typeof entry === "function" || typeof entry === "symbol" ||
        typeof entry === "bigint" || (typeof entry === "number" && !Number.isFinite(entry)))
      throw new Error("runtime hooks must return JSON");
    return entry;
  });
  if (typeof result !== "string") throw new Error("runtime hook returned no JSON");
  if (result.length > globalThis.__ct_runtime_limit__) throw new Error("runtime size limit");
  return result;
})()
"#;

pub(crate) fn call_runtime_hook<R: serde::de::DeserializeOwned>(
    vm: &PluginVm,
    method: &'static str,
    args: &[serde_json::Value],
    deadline: std::time::Instant,
) -> Result<R, RuntimeFailure> {
    call_runtime_hook_with_clock(
        vm,
        method,
        args,
        deadline,
        std::sync::Arc::new(std::time::Instant::now),
    )
}

type HookClock = std::sync::Arc<dyn Fn() -> std::time::Instant + Send + Sync>;

fn call_runtime_hook_with_clock<R: serde::de::DeserializeOwned>(
    vm: &PluginVm,
    method: &'static str,
    args: &[serde_json::Value],
    deadline: std::time::Instant,
    now: HookClock,
) -> Result<R, RuntimeFailure> {
    let mut timing = timing::HookTiming::new(method);
    let _bundle_reader = vm.bundle_access.read();
    crate::agent_runtime::validate_runtime_size(&args, MAX_RUNTIME_VALUE_BYTES)?;
    let args_json =
        serde_json::to_string(args).map_err(|e| failure(RuntimeFailureCode::InvalidSchema, e))?;
    timing.bytes = args_json.len();
    if let Some(reply) = args.get(3) {
        log::debug!(
            "[runtime-hook] method={method} reply_bytes={}",
            timing::serialized_bytes(reply)
        );
    }
    timing.enter("vm_wait");
    let remaining = deadline.saturating_duration_since(now());
    let mut runtime_inner = vm
        .runtime_inner
        .try_lock_for(remaining)
        .ok_or_else(|| failure(RuntimeFailureCode::Deadline, "runtime VM lock deadline"))?;
    if now() >= deadline {
        return Err(failure(
            RuntimeFailureCode::Deadline,
            "runtime hook preparation deadline",
        ));
    }
    if runtime_inner.is_none() {
        timing.enter("initialize");
        let context = create_runtime_context(vm, deadline, now.clone());
        if now() >= deadline {
            return Err(failure(
                RuntimeFailureCode::Deadline,
                "runtime hook initialization deadline",
            ));
        }
        *runtime_inner = Some(context?);
    }
    timing.enter("execute");
    let inner = runtime_inner
        .as_ref()
        .ok_or_else(|| failure(RuntimeFailureCode::HookFailed, "runtime context missing"))?;
    let interrupt_now = now.clone();
    inner
        ._rt
        .set_interrupt_handler(Some(Box::new(move || interrupt_now() >= deadline)));
    let mut context_unusable = false;
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        inner._ctx.with(|ctx| -> Result<String, RuntimeFailure> {
            let globals = ctx.globals();
            let mut setup = |e: rquickjs::CaughtError<'_>| {
                context_unusable = true;
                failure(
                    RuntimeFailureCode::HookFailed,
                    format!("{method} setup: {e:?}"),
                )
            };
            globals
                .set("__ct_runtime_method__", method)
                .catch(&ctx)
                .map_err(&mut setup)?;
            globals
                .set("__ct_runtime_args__", args_json)
                .catch(&ctx)
                .map_err(&mut setup)?;
            globals
                .set("__ct_runtime_limit__", MAX_RUNTIME_VALUE_BYTES as u32)
                .catch(&ctx)
                .map_err(&mut setup)?;
            let result = eval_source::<String, _>(&ctx, INVOKE)
                .catch(&ctx)
                .map_err(|e| {
                    #[cfg(debug_assertions)]
                    if let rquickjs::CaughtError::Value(value) = &e {
                        if let Some(object) = value.as_object() {
                            log::debug!(
                                "[runtime-hook-refusal] method={} code={:?} message={:?}",
                                method,
                                object.get::<_, Option<String>>("code").ok().flatten(),
                                object.get::<_, Option<String>>("message").ok().flatten()
                            );
                        }
                    }
                    failure(RuntimeFailureCode::HookFailed, format!("{method}: {e:?}"))
                });
            let _ = globals.remove("__ct_runtime_args__");
            result
        })
    }));
    inner._rt.set_interrupt_handler(None);
    inner._rt.set_gc_threshold(RUNTIME_GC_THRESHOLD);
    if context_unusable {
        let usage = inner._rt.memory_usage();
        if let Ok(Err(error)) = &outcome {
            log::warn!(
                "[runtime-hook] plugin={} method={method} context reset: {} heap={}/{}",
                vm.plugin_id(),
                error.message,
                usage.malloc_size,
                usage.malloc_limit
            );
        }
        *runtime_inner = None;
    }
    if now() >= deadline {
        return Err(failure(
            RuntimeFailureCode::Deadline,
            "runtime hook execution deadline",
        ));
    }
    let result =
        outcome.map_err(|_| failure(RuntimeFailureCode::HookFailed, "runtime hook panicked"))??;
    timing.enter("decode");
    if result.len() > MAX_RUNTIME_VALUE_BYTES {
        return Err(failure(
            RuntimeFailureCode::SizeLimit,
            "runtime hook output too large",
        ));
    }
    serde_json::from_str(&result).map_err(|e| failure(RuntimeFailureCode::InvalidSchema, e))
}

fn failure(code: RuntimeFailureCode, message: impl std::fmt::Display) -> RuntimeFailure {
    RuntimeFailure::new(code, message.to_string())
}

fn create_runtime_context(
    vm: &PluginVm,
    deadline: std::time::Instant,
    now: HookClock,
) -> Result<Inner, RuntimeFailure> {
    let source = vm
        .inner
        .try_lock_for(deadline.saturating_duration_since(now()))
        .ok_or_else(|| failure(RuntimeFailureCode::Deadline, "runtime source lock deadline"))?;
    let bundle_source = source.bundle_source.clone();
    let manifest_json = source.manifest_json.clone();
    drop(source);
    let rt = Runtime::new().map_err(|e| failure(RuntimeFailureCode::HookFailed, e))?;
    rt.set_memory_limit(RUNTIME_MEMORY_LIMIT);
    rt.set_max_stack_size(1024 * 1024);
    rt.set_interrupt_handler(Some(Box::new(move || now() >= deadline)));
    let ctx = Context::full(&rt).map_err(|e| failure(RuntimeFailureCode::HookFailed, e))?;
    let manifest_literal = serde_json::to_string(&manifest_json.as_deref().unwrap_or("null"))
        .map_err(|e| failure(RuntimeFailureCode::InvalidSchema, e))?;
    let platform_literal = serde_json::to_string(crate::provider::current_platform_tag())
        .map_err(|e| failure(RuntimeFailureCode::InvalidSchema, e))?;
    let bootstrap = format!(
        r#"globalThis.host = (function(){{let denied;Object.defineProperty(globalThis,'__ct_runtime_denied_binding__',{{get:()=>denied}});return new Proxy(Object.freeze({{manifest:()=>JSON.parse({manifest_literal}),platform:()=>{platform_literal}}}),{{get:(target,key)=>{{if(key in target)return target[key];denied=String(key);throw new Error('runtime hook host binding unavailable: '+denied);}}}});}})();
        globalThis.__plugin__=(function(){{const module={{exports:{{}}}};const exports=module.exports;{bundle_source};return module.exports.default || module.exports;}})();"#
    );
    ctx.with(|ctx| -> Result<(), RuntimeFailure> {
        eval_source::<(), _>(&ctx, bootstrap)
            .catch(&ctx)
            .map_err(|e| {
                failure(
                    RuntimeFailureCode::HookFailed,
                    format!("runtime bundle: {e:?}"),
                )
            })?;
        let denied: Option<String> = ctx
            .globals()
            .get("__ct_runtime_denied_binding__")
            .map_err(|e| failure(RuntimeFailureCode::HookFailed, e))?;
        if let Some(binding) = denied {
            return Err(failure(
                RuntimeFailureCode::HookFailed,
                format!("runtime bundle host binding unavailable: {binding}"),
            ));
        }
        #[cfg(test)]
        {
            let gates = rquickjs::Object::new(ctx.clone())
                .map_err(|e| failure(RuntimeFailureCode::HookFailed, e))?;
            super::super::call_gate::install(&gates, &vm.sandbox)
                .map_err(|e| failure(RuntimeFailureCode::HookFailed, e))?;
            ctx.globals()
                .set("__runtime_test_gate__", gates)
                .map_err(|e| failure(RuntimeFailureCode::HookFailed, e))?;
        }
        Ok(())
    })?;
    rt.set_interrupt_handler(None);
    Ok(Inner {
        _ctx: ctx,
        _rt: rt,
        bundle_source,
        manifest_json,
    })
}

#[cfg(test)]
mod tests;
