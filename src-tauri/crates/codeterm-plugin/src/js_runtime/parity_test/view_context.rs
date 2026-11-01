use crate::js_runtime::{JsViewProvider, PluginVm};
use crate::view_host::{ViewCallContext, ViewError, ViewProvider};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

struct Native;
#[async_trait]
impl ViewProvider for Native {
    fn id(&self) -> &str {
        "scoped-view"
    }
    async fn call(
        &self,
        method: &str,
        args: Value,
        context: &ViewCallContext,
    ) -> Result<Value, ViewError> {
        if args["result_owner"].as_str() != context.owner.as_deref() {
            return Err(ViewError::Call("foreign result".into()));
        }
        Ok(json!({"method":method,"context":context}))
    }
}

#[tokio::test]
async fn native_and_js_view_callbacks_use_host_context_before_owner_scoped_lookup() {
    let vm = Arc::new(
        PluginVm::from_source(
            "scoped-view",
            r#"module.exports.default={viewCall(method,args,context){
        if(args.result_owner!==context.owner)throw Error('foreign result');
        return {method:method,context:context};
    }}"#
            .into(),
        )
        .unwrap(),
    );
    let js = JsViewProvider::new(vm, "scoped-view");
    let mut context = ViewCallContext::panel("scoped-view");
    context.owner = Some("owner-one".into());
    context.tab = Some("view-tab".into());
    context.surface = Some("board".into());
    context.instance = Some("instance".into());
    context.workspace = Some("workspace".into());
    context.frame = Some("non-secret-frame".into());
    let args =
        json!({"result_owner":"owner-one","context":{"owner":"attacker"},"actor":"attacker"});
    assert_eq!(
        Native.call("read", args.clone(), &context).await.unwrap(),
        js.call("read", args, &context).await.unwrap()
    );
    for provider in [&Native as &dyn ViewProvider, &js as &dyn ViewProvider] {
        assert!(provider
            .call(
                "read",
                json!({"result_owner":"other-owner","owner":"owner-one"}),
                &context
            )
            .await
            .is_err());
    }
    assert!(crate::view_host::current_view_call_context("scoped-view").is_none());
}
