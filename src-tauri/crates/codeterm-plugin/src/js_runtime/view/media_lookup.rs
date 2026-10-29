use super::*;
use crate::{converter::ConversionCancellation,media_delivery::MediaFile,view_host::ViewCallContext};
use std::time::{Duration,Instant};
struct LookupLifetime(ConversionCancellation);
impl Drop for LookupLifetime {fn drop(&mut self){self.0.cancel();}}
pub(super) fn deadline()->Instant {
    let bound=Instant::now()+Duration::from_secs(5);
    codeterm_decision::physical::deadline().map_or(bound,|deadline|deadline.min(bound))
}
pub(super) async fn lookup(vm:Arc<PluginVm>,plugin:String,id:String,context:ViewCallContext,cancellation:ConversionCancellation,deadline:Instant)->Result<MediaFile,ViewError>{
    let _lifetime=LookupLifetime(cancellation.clone());
    let observer=codeterm_decision::physical::current();
    tokio::task::spawn_blocking(move ||{let _scope=observer.map(codeterm_decision::physical::RequestScope::enter);run(&vm,&plugin,&id,&context,cancellation,deadline)}).await.map_err(|_|ViewError::Call("media task failed".into()))?
}
fn run(vm:&PluginVm,plugin:&str,id:&str,context:&ViewCallContext,cancellation:ConversionCancellation,deadline:Instant)->Result<MediaFile,ViewError>{
    if cancellation.is_cancelled()||Instant::now()>=deadline{return Err(ViewError::Call("media lookup cancelled or expired".into()));}
    let argument=serde_json::to_value(context).map_err(|_|ViewError::Call("invalid media context".into()))?;
    crate::view_host::with_view_call_context(plugin,context,||super::super::vm::call_json_interruptible(vm,"mediaFile",&[Value::String(id.into()),argument],cancellation,deadline).map_err(|_|ViewError::Call("media file refused".into())))
}
pub(super) async fn lookup_run(vm:Arc<PluginVm>,plugin:String,id:String,context:crate::media_delivery::RunMediaLookupContext,cancellation:ConversionCancellation,deadline:Instant)->Result<MediaFile,ViewError>{
    if crate::view_host::has_view_call_context(){return Err(ViewError::Call("durable media lookup denied".into()));}
    let _lifetime=LookupLifetime(cancellation.clone());
    let observer=codeterm_decision::physical::current().filter(|observer|!observer.effects_allowed()).ok_or_else(||ViewError::Call("durable media lookup requires observation-only scope".into()))?;
    tokio::task::spawn_blocking(move ||{let _scope=codeterm_decision::physical::RequestScope::enter(observer);run_durable(&vm,&plugin,&id,&context,cancellation,deadline)}).await.map_err(|_|ViewError::Call("media task failed".into()))?
}
fn run_durable(vm:&PluginVm,plugin:&str,id:&str,context:&crate::media_delivery::RunMediaLookupContext,cancellation:ConversionCancellation,deadline:Instant)->Result<MediaFile,ViewError>{
    if codeterm_decision::physical::current().is_none_or(|observer|observer.effects_allowed())||codeterm_decision::physical::effects_allowed(){return Err(ViewError::Call("durable media lookup requires observation-only scope".into()));}
    if crate::view_host::has_view_call_context()||!context.valid_for(plugin)||cancellation.is_cancelled()||Instant::now()>=deadline{return Err(ViewError::Call("durable media lookup denied".into()));}
    let argument=serde_json::to_value(context).map_err(|_|ViewError::Call("invalid media context".into()))?;
    crate::media_delivery::with_run_media_lookup(plugin,||super::super::vm::call_json_interruptible(vm,"mediaFile",&[Value::String(id.into()),argument],cancellation,deadline).map_err(|_|ViewError::Call("media file refused".into())))
}
#[cfg(test)]
mod tests;
