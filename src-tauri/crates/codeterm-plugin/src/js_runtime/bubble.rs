use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;

mod cache;
use serde_json::Value;

use super::vm::{call_json, PluginVm};
use crate::bubble::{BubbleData, BubbleError, BubbleProvider};

pub struct JsBubbleProvider {
    vm: Arc<PluginVm>,
    id: String,
    cache: Arc<Mutex<cache::BubbleCache<Option<BubbleData>>>>,
}

impl JsBubbleProvider {
    pub fn new(vm: Arc<PluginVm>, id: impl Into<String>) -> Self {
        Self {
            vm,
            id: id.into(),
            cache: Arc::new(Mutex::new(cache::BubbleCache::default())),
        }
    }
}

impl JsBubbleProvider {
    fn request(&self, ctx: Value) -> (Option<BubbleData>, Option<tokio::task::JoinHandle<()>>) {
        let key = ctx
            .get("cwd")
            .and_then(Value::as_str)
            .or_else(|| ctx.get("tabId").and_then(Value::as_str))
            .unwrap_or("")
            .to_owned();
        let invalidated = ctx.get("refresh").and_then(Value::as_bool).unwrap_or(false);
        let (value, refresh) = self.cache.lock().request(&key, invalidated);
        let worker = if refresh {
            let vm = self.vm.clone();
            let cache = self.cache.clone();
            let id = self.id.clone();
            Some(tokio::task::spawn_blocking(move || loop {
                super::vm::grant_ctx_cwd(&vm, &ctx);
                let result = call_json::<Option<BubbleData>>(&vm, "statusBubble", &[ctx.clone()]);
                let succeeded = result.is_ok();
                if let Err(error) = &result {
                    log::debug!("[plugin-bubble] {id} refresh failed: {error:?}");
                }
                let again = cache.lock().finish(&key, result.ok());
                if succeeded {
                    if let Some(service) = vm.service() {
                        service.contributions_changed(&format!("status_bubble:{id}"));
                    }
                }
                if !again {
                    break;
                }
            }))
        } else {
            None
        };
        (value.flatten(), worker)
    }
}

#[async_trait]
impl BubbleProvider for JsBubbleProvider {
    fn id(&self) -> &str {
        &self.id
    }

    async fn bubble(&self, ctx: Value) -> Result<Option<BubbleData>, BubbleError> {
        Ok(self.request(ctx).0)
    }
}

#[cfg(test)]
mod tests;
