use serde::Serialize;
use std::cell::RefCell;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMediaLookupOrigin {
    DurableRun,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunMediaLookupContext {
    pub origin: RunMediaLookupOrigin,
    pub plugin: String,
    pub workspace: String,
    pub team: Option<String>,
    pub owner: String,
    pub run: String,
    pub producer_operation: String,
    pub producer_port: String,
    pub receiver_node: String,
}
impl RunMediaLookupContext {
    pub fn valid_for(&self, plugin: &str) -> bool {
        self.plugin == plugin
            && super::valid_plugin_id(plugin)
            && [
                &self.workspace,
                &self.owner,
                &self.run,
                &self.producer_operation,
                &self.producer_port,
                &self.receiver_node,
            ]
            .into_iter()
            .all(|value| !value.is_empty() && value.len() <= 256)
            && self
                .team
                .as_ref()
                .is_none_or(|team| !team.is_empty() && team.len() <= 256)
    }
}
thread_local! {static LOOKUP:RefCell<Option<String>>=const{RefCell::new(None)};}
struct LookupScope(Option<String>);
impl Drop for LookupScope {
    fn drop(&mut self) {
        LOOKUP.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}
pub(crate) fn run_media_lookup_active() -> bool {
    LOOKUP.with(|slot| slot.borrow().is_some())
}
pub(crate) fn with_run_media_lookup<T>(plugin: &str, work: impl FnOnce() -> T) -> T {
    let _scope = LookupScope(LOOKUP.with(|slot| slot.replace(Some(plugin.into()))));
    work()
}
