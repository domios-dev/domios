use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::agent_task::{AgentTaskInput, AgentTaskRequest, AgentTaskResult, AgentTaskStatus};
use crate::chatbackend_provider::{ChatBackendError, ChatBackendProvider};
use crate::converter::ConversionCancellation;
use crate::provider::ModelInfo;
use codeterm_types::agent::chat_types::StructuredChat;

struct Native;
impl ChatBackendProvider for Native {
    fn id(&self) -> &str {
        "fixture"
    }
    fn supports_agent_tasks(&self) -> bool {
        true
    }
    fn agent_task_media(&self) -> Vec<crate::agent_task::AgentTaskMediaKind> {
        vec![crate::agent_task::AgentTaskMediaKind::Image]
    }
    fn run_agent_task(&self, request: &AgentTaskRequest) -> Result<AgentTaskResult, String> {
        crate::agent_task::validate_input(&request.input)?;
        crate::agent_task::validate_media_capability(&request.input, &self.agent_task_media())?;
        Ok(AgentTaskResult {
            status: AgentTaskStatus::Done,
            output: serde_json::json!({"task":request.input.task,"input":request.input.input,"media":request.input.media}),
            error: None,
        })
    }
    fn abort_agent_task(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
        Ok("fixture".into())
    }
    fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }
    fn poll(&self, _: &str, _: Option<&str>) -> Result<StructuredChat, ChatBackendError> {
        Ok(StructuredChat::default())
    }
    fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }
    fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError> {
        Ok(Vec::new())
    }
}

#[test]
fn parity_agent_tasks_preserve_input_and_structured_result() {
    let manifest_json = r#"{"id":"task-fixture","displayName":"fixture","version":"0.1.0","capabilities":{"chatBackend":{"agentTasks":true}}}"#;
    let manifest = serde_json::from_str(manifest_json).unwrap();
    let vm = super::super::PluginVm::from_source_with("task-fixture", r#"module.exports.default = {
        agentTaskMedia() { return ['image']; },
        runAgentTask(request) { return {status:'done', output:{task:request.task,input:request.input,media:request.media || []}}; },
        abortAgentTask(id) { return null; }
    };"#.into(), Some(manifest_json.into())).unwrap();
    let js = super::super::JsChatBackend::from_manifest(Arc::new(vm), &manifest);
    let request = AgentTaskRequest {
        input: AgentTaskInput {
            run_id: "fixture-run".into(),
            tab_id: "fixture-tab".into(),
            workspace_id: "fixture-workspace".into(),
            profile: "fixture-profile".into(),
            task: "  opaque task\n".into(),
            input: "opaque input".into(),
            media: {
                use sha2::{Digest, Sha256};
                let bytes = include_bytes!("../../../../../icons/32x32.png").to_vec();
                vec![crate::agent_task::AgentTaskMedia {
                    kind: crate::agent_task::AgentTaskMediaKind::Image,
                    content_type: "image/png".into(),
                    file_name: "fixture.png".into(),
                    sha256: format!("{:x}", Sha256::digest(&bytes)),
                    bytes,
                    lineage: crate::blueprint_step::InputLineage {
                        plugin: "fixture".into(),
                        operation: "operation".into(),
                        port: "image".into(),
                    },
                }]
            },
            timeout_ms: 1000,
            max_tokens: 1000,
            max_turns: 2,
        },
        deadline: Instant::now() + Duration::from_secs(2),
        cancellation: ConversionCancellation::default(),
    };
    assert_eq!(js.supports_agent_tasks(), Native.supports_agent_tasks());
    assert_eq!(js.agent_task_media(), Native.agent_task_media());
    assert_eq!(
        js.run_agent_task(&request).unwrap(),
        Native.run_agent_task(&request).unwrap()
    );
    assert!(js.abort_agent_task("fixture-run").is_ok());
    let text_vm = super::super::PluginVm::from_source_with("task-fixture", "module.exports.default={runAgentTask(){throw Error('must not dispatch media');},abortAgentTask(){}};".into(), Some(manifest_json.into())).unwrap();
    let text_backend = super::super::JsChatBackend::from_manifest(Arc::new(text_vm), &manifest);
    assert!(text_backend.agent_task_media().is_empty());
    assert!(text_backend
        .run_agent_task(&request)
        .unwrap_err()
        .contains("media modality"));
    let mut invalid = request;
    invalid.input.max_turns = 0;
    assert!(js.run_agent_task(&invalid).is_err());
    assert!(Native.run_agent_task(&invalid).is_err());
}

#[test]
fn agent_task_methods_without_a_capability_are_refused() {
    let manifest_json = r#"{"id":"task-fixture","displayName":"fixture","version":"0.1.0","capabilities":{"chatBackend":true}}"#;
    let manifest = serde_json::from_str(manifest_json).unwrap();
    let vm = super::super::PluginVm::from_source_with(
        "task-fixture",
        "module.exports.default = {runAgentTask(){},abortAgentTask(){}};".into(),
        Some(manifest_json.into()),
    )
    .unwrap();
    assert!(
        !super::super::JsChatBackend::from_manifest(Arc::new(vm), &manifest).supports_agent_tasks()
    );
}
