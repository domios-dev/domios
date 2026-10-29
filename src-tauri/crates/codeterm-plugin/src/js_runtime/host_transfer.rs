use super::host_api::PluginSandbox;
use crate::binary_transfer::{self as transfer, *};
use std::{collections::BTreeMap,path::{Path,PathBuf},sync::{Arc,Weak}};

struct TransferConfig {pub descriptor:BinaryTransferDescriptor,pub bundle:PathBuf,pub permissions:super::manifest::PermissionsConfig}
#[derive(Default)]
pub(super) struct TransferRuntime {config:Option<TransferConfig>,pub(super) session:parking_lot::Mutex<Option<Arc<TransferSession>>>}
impl PluginSandbox {
    pub(super) fn with_binary_transfer(mut self, descriptor: Option<crate::binary_transfer::BinaryTransferDescriptor>, bundle: &Path, permissions: &super::manifest::PermissionsConfig) -> Self {
        self.transfer.config = descriptor.map(|descriptor| super::host_transfer::TransferConfig {descriptor, bundle: bundle.to_path_buf(), permissions: permissions.clone()});
        self
    }

}
pub(super) fn install_handles(host:&rquickjs::Object<'_>,sandbox:&Arc<PluginSandbox>)->std::result::Result<(),super::vm::VmError>{
    use rquickjs::function::Func;
    let sb=sandbox.clone();host.set("transferStart",Func::new(move |json:String|start(&sb,&json))).map_err(|error|super::vm::VmError::Script(format!("set transferStart: {error}")))?;
    let sb=sandbox.clone();host.set("transferAbort",Func::new(move |job:String|abort(&sb,&job))).map_err(|error|super::vm::VmError::Script(format!("set transferAbort: {error}")))?;
    let sb=sandbox.clone();host.set("transferBindInput",Func::new(move |json:String|bind_start(&sb,&json))).map_err(|error|super::vm::VmError::Script(format!("set transferBindInput: {error}")))?;
    Ok(())
}
struct Authority {sandbox:Weak<PluginSandbox>}
impl Authority {
    fn sandbox(&self)->Result<Arc<PluginSandbox>> {self.sandbox.upgrade().ok_or(TransferRefusal::NotAllowed)}
    fn permissions(&self,sandbox:&PluginSandbox)->Result<super::manifest::PermissionsConfig> {
        let config=sandbox.transfer.config.as_ref().ok_or(TransferRefusal::NotAllowed)?;
        let mut permissions=config.permissions.clone();
        super::grants::apply_persisted(&mut permissions,&config.bundle);
        Ok(permissions)
    }
}
impl TransferAuthority for Authority {
    fn authorize(&self,url:&reqwest::Url)->Result<()> {
        let sandbox=self.sandbox()?;
        if !exact_grant(url,&self.permissions(&sandbox)?.network.allow) {return Err(TransferRefusal::NotAllowed);}
        Ok(())
    }
    fn readable(&self,path:&Path)->bool {self.sandbox().is_ok_and(|sandbox|sandbox.can_read(&path.to_string_lossy()))}
    fn credentials(&self,credential:&Credential)->Result<BTreeMap<String,String>> {
        let sandbox=self.sandbox()?;
        if !self.permissions(&sandbox)?.secrets {return Err(TransferRefusal::NotAllowed);}
        match credential {
            Credential::Secret {name,header,prefix}=> {
                if name.is_empty() || name.len()>128 || header.is_empty() || header.len()>128 || prefix.as_ref().is_some_and(|prefix|prefix.len()>64) {return Err(TransferRefusal::InvalidRequest);}
                let value=sandbox.service().and_then(|service|service.secret_get(sandbox.plugin_id(),name)).ok_or(TransferRefusal::NotAllowed)?;
                if value.len()>8192 {return Err(TransferRefusal::NotAllowed);}
                Ok(BTreeMap::from([(header.clone(),format!("{}{value}",prefix.as_deref().unwrap_or("")))]))
            },
            Credential::Declared {id,headers}=> {
                if id.len()>128 || headers.len()>8 || headers.iter().any(|(key,value)|key.len()>128||value.len()>1024) {return Err(TransferRefusal::InvalidRequest);}
                let prepared=super::host_credentials::inject(sandbox.credentials(),&serde_json::json!({"credential":{"id":id,"headers":headers}}).to_string()).map_err(|_|TransferRefusal::NotAllowed)?;
                let value:serde_json::Value=serde_json::from_str(&prepared).map_err(|_|TransferRefusal::NotAllowed)?;
                serde_json::from_value(value.get("headers").cloned().unwrap_or(serde_json::json!({}))).map_err(|_|TransferRefusal::NotAllowed)
            }
        }
    }
}
pub(super) fn session(sandbox:&Arc<PluginSandbox>)->Result<Arc<TransferSession>> {
    let mut current=sandbox.transfer.session.lock();
    if let Some(session)=current.as_ref(){return Ok(session.clone());}
    let config=sandbox.transfer.config.as_ref().ok_or(TransferRefusal::NotAllowed)?;
    let authority=Arc::new(Authority {sandbox:Arc::downgrade(sandbox)});
    let session=TransferSession::new(sandbox.plugin_id(),config.descriptor.clone(),authority)?;
    *current=Some(session.clone());
    Ok(session)
}
pub(super) fn activate(sandbox:&Arc<PluginSandbox>) {
    {
        let mut current=sandbox.transfer.session.lock();
        if current.as_ref().is_some_and(|session|session.check().is_err()){*current=None;}
    }
    if let Ok(session)=session(sandbox) {transfer::activate(&session);}
}
pub(super) fn reload(sandbox:&Arc<PluginSandbox>){
    let owned=sandbox.transfer.session.lock().as_ref().is_some_and(|session|crate::binary_transfer::is_current(session));
    revoke(sandbox);
    if owned{activate(sandbox);}
}
pub(super) fn revoke(sandbox:&PluginSandbox) {
    if let Some(session)=sandbox.transfer.session.lock().as_ref(){session.revoke();super::host_exec::drop_scoped_json_jobs(sandbox.plugin_id(),session.epoch());}
}
fn failure(error:TransferRefusal)->String {serde_json::to_string(&TransferFailure {error,request_may_have_started:false,status:None}).unwrap_or_else(|_|"{}".into())}
pub(super) fn start(sandbox:&Arc<PluginSandbox>,json:&str)->String {
    if crate::media_delivery::run_media_lookup_active()||!codeterm_decision::physical::effects_allowed(){return failure(TransferRefusal::NotAllowed);}
    if json.len()>128*1024 {return failure(TransferRefusal::TooLarge);}
    let request:TransferRequest=match serde_json::from_str(json){Ok(value)=>value,Err(_)=>return failure(TransferRefusal::InvalidRequest)};
    let session=match session(sandbox){Ok(value)=>value,Err(error)=>return failure(error)};
    if !crate::binary_transfer::is_current(&session){return failure(TransferRefusal::NotAllowed);}
    let deadline=match session.deadline(&request){Ok(value)=>value,Err(error)=>return failure(error)};
    let abort=TransferAbort::default();
    let cancel=abort.clone();
    let plugin=session.plugin_id().to_owned();
    let epoch=session.epoch().to_owned();
    let observer=codeterm_decision::physical::current();
    match super::host_exec::start_scoped_json_job(&plugin,&epoch,deadline,Arc::new(move ||cancel.abort()),serde_json::json!({"error":"deadline","requestMayHaveStarted":true,"status":null}),move || {
        let _scope=observer.map(codeterm_decision::physical::RequestScope::enter);
        match transfer::execute_until(session,request,abort,deadline) {
            Ok(receipt)=>serde_json::to_string(&receipt).unwrap_or_else(|_|failure(TransferRefusal::Unavailable)),
            Err(error)=>serde_json::to_string(&error).unwrap_or_else(|_|failure(TransferRefusal::Unavailable)),
        }
    }) {Ok(job_id)=>serde_json::json!({"jobId":job_id}).to_string(),Err(_)=>failure(TransferRefusal::Unavailable)}
}
pub(super) fn abort(sandbox:&Arc<PluginSandbox>,job:&str)->bool {
    if !codeterm_decision::physical::effects_allowed(){return false;}
    let Ok(session)=session(sandbox) else{return false};
    super::host_exec::abort_scoped_json_job(sandbox.plugin_id(),session.epoch(),job)
}

pub(super) fn deactivate(plugin:&str) {
    let session=crate::binary_transfer::live_session(plugin);
    transfer::deactivate(plugin);
    if let Some(session)=session {super::host_exec::drop_scoped_json_jobs(plugin,session.epoch());}
}

#[cfg(test)]
pub(super) fn fixture_session(sandbox:&Arc<PluginSandbox>,root:PathBuf,network:Arc<dyn crate::binary_transfer::TestNetwork>)->Arc<TransferSession> {
    let descriptor=sandbox.transfer.config.as_ref().unwrap().descriptor.clone();
    let session=TransferSession::at_root(sandbox.plugin_id(),root,descriptor,Arc::new(Authority {sandbox:Arc::downgrade(sandbox)})).unwrap();
    *session.test_network.lock()=Some(network);
    *sandbox.transfer.session.lock()=Some(session.clone());
    transfer::activate(&session);
    session
}

#[derive(serde::Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct BindInput {handle:String,destination_origin:String}
pub(super) fn bind_start(sandbox:&Arc<PluginSandbox>,json:&str)->String {
    if crate::media_delivery::run_media_lookup_active()||!codeterm_decision::physical::effects_allowed(){return failure(TransferRefusal::NotAllowed);}
    if json.len()>4096{return failure(TransferRefusal::TooLarge);}
    let request:BindInput=match serde_json::from_str(json){Ok(value)=>value,Err(_)=>return failure(TransferRefusal::InvalidRequest)};
    if !crate::media_delivery::valid_token(&request.handle){return failure(TransferRefusal::InvalidRequest);}
    let Some(context)=crate::view_host::current_view_call_context(sandbox.plugin_id()) else{return failure(TransferRefusal::NotAllowed)};
    let session=match session(sandbox){Ok(value)=>value,Err(error)=>return failure(error)};
    if !crate::binary_transfer::is_current(&session){return failure(TransferRefusal::NotAllowed);}
    let Some(service)=sandbox.service() else{return failure(TransferRefusal::Unavailable)};
    let requested=std::time::Instant::now()+std::time::Duration::from_millis(sandbox.transfer.config.as_ref().map_or(0,|config|config.descriptor.timeout_ms));
    let deadline=codeterm_decision::physical::deadline().map_or(requested,|bound|bound.min(requested));
    let control=InputPreparation{abort:TransferAbort::default(),deadline};let cancel=control.abort.clone();
    let plugin=session.plugin_id().to_owned();let epoch=session.epoch().to_owned();let owner=plugin.clone();
    let observer=codeterm_decision::physical::current();
    match super::host_exec::start_scoped_json_job(&plugin,&epoch,deadline,Arc::new(move ||cancel.abort()),serde_json::json!({"error":"deadline","requestMayHaveStarted":false,"status":null}),move ||{
        let _scope=observer.map(codeterm_decision::physical::RequestScope::enter);
        match service.bind_view_transfer_input(&owner,&context,&request.handle,&request.destination_origin,&control){
            Ok(grant)=>serde_json::to_string(&grant).unwrap_or_else(|_|failure(TransferRefusal::Unavailable)),
            Err(error)=>failure(error),
        }
    }){Ok(job)=>serde_json::json!({"jobId":job}).to_string(),Err(_)=>failure(TransferRefusal::Unavailable)}
}
