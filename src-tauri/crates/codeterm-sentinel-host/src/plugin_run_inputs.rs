use crate::pipeline::plugin;
use codeterm_plugin::{
    binary_transfer::*, blueprint_step::*, graph_nodes::GraphNodeRegistry,
    view_host::ViewRegistryHandle,
};
use codeterm_service_core::service::ServiceContext;
use std::sync::{Arc, Weak};

pub(crate) struct Inputs(pub Weak<ServiceContext>);

impl RunInputHost for Inputs {
    fn bind(
        &self,
        consumer: &str,
        context: &StepInputContext,
        handle: &str,
        destination: &str,
        control: &InputPreparation,
    ) -> Result<TransferInputGrant> {
        let service = self.0.upgrade().ok_or(TransferRefusal::Unavailable)?;
        let ledger = plugin::ledger(&service).map_err(|_| TransferRefusal::Unavailable)?;
        let steps = service.ext::<Arc<GraphNodeRegistry>>();
        let current = steps
            .step(&context.request.node_type)
            .ok_or(TransferRefusal::NotAllowed)?;
        let valid = || {
            let record = ledger
                .get(&context.request.context.operation)
                .map_err(|_| TransferRefusal::Unavailable)?
                .ok_or(TransferRefusal::NotAllowed)?;
            let mux = service.mux.read();
            let scope = &context.request.context;
            if context.plugin != consumer
                || !context.submitting
                || record.plugin != consumer
                || context.cancellation.is_cancelled()
                || std::time::Instant::now() >= context.deadline
                || record.request != context.request
                || record.state != OperationState::Submitting
                || record.cancel_requested
                || mux.find_workspace_for_tab(&scope.owner).as_deref()
                    != Some(scope.scope.workspace.as_str())
                || mux
                    .get_tab(&scope.owner)
                    .is_none_or(|owner| owner.team_id != scope.scope.team)
                || steps.step(&context.request.node_type).is_none_or(|step| {
                    step.plugin != consumer || !Arc::ptr_eq(&step.adapter, &current.adapter)
                })
            {
                return Err(TransferRefusal::NotAllowed);
            }
            Ok(())
        };
        valid()?;
        let kind = context
            .request
            .inputs
            .values()
            .flatten()
            .find_map(|value| match value {
                PortValue::Media {
                    handle: declared,
                    media,
                } if declared == handle => Some(*media),
                _ => None,
            })
            .ok_or(TransferRefusal::NotAllowed)?;
        let media = plugin::media(&service).map_err(|_| TransferRefusal::Unavailable)?;
        let source = media
            .resolve_input(&context.request.context, consumer, handle, kind)
            .map_err(|_| TransferRefusal::NotAllowed)?;
        let views = service.ext::<ViewRegistryHandle>();
        let producer_epoch = views
            .registration_id(&source.plugin)
            .ok_or(TransferRefusal::NotAllowed)?;
        let files = RunMediaFiles {
            registry: views.clone(),
            runtime: service.rt.clone(),
        };
        let opened = files
            .open(
                &source.lookup,
                &source.media_id,
                kind,
                Some(&source.fingerprint),
            )
            .map_err(|_| TransferRefusal::NotAllowed)?;
        let identity = FileIdentity {
            size: opened.size,
            sha256: source.fingerprint,
        };
        let prepared = prepare_plugin_input_bounded(
            consumer,
            destination,
            opened.file,
            &opened.file_name,
            &opened.content_type,
            Some(&identity),
            control,
        )?;
        valid()?;
        if views.registration_id(&source.plugin).as_deref() != Some(producer_epoch.as_str()) {
            return Err(TransferRefusal::NotAllowed);
        }
        prepared.commit()
    }
}
