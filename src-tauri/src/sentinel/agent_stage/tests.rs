use super::*;

#[test]
fn blueprint_agent_deadline_and_status_are_bounded() {
    let now = Instant::now();
    assert_eq!(
        effective_deadline(1000, Some(100), now),
        now + Duration::from_millis(100)
    );
    for (status, expected) in [
        (AgentTaskStatus::Done, (true, false)),
        (AgentTaskStatus::Failed, (false, false)),
        (AgentTaskStatus::Cancelled, (false, true)),
        (AgentTaskStatus::DeadlineExceeded, (false, true)),
    ] {
        assert_eq!(
            result_fields(&AgentTaskResult {
                status,
                output: serde_json::json!({"fixture":1}),
                error: None
            }),
            expected
        );
    }
}

struct Fixture {
    started: mpsc::Sender<()>,
    aborted: mpsc::Sender<String>,
}
impl ChatBackendProvider for Fixture {
    fn id(&self) -> &str {
        "fixture"
    }
    fn supports_agent_tasks(&self) -> bool {
        true
    }
    fn run_agent_task(&self, request: &AgentTaskRequest) -> Result<AgentTaskResult, String> {
        self.started.send(()).unwrap();
        while !request.cancellation.is_cancelled() {
            let generation = request.cancellation.generation();
            if request.cancellation.is_cancelled() {
                break;
            }
            request
                .cancellation
                .wait_for_change(generation, std::time::Duration::from_secs(2));
        }
        Ok(AgentTaskResult {
            status: AgentTaskStatus::Cancelled,
            output: serde_json::Value::Null,
            error: None,
        })
    }
    fn abort_agent_task(&self, run_id: &str) -> Result<(), String> {
        self.aborted.send(run_id.into()).unwrap();
        Ok(())
    }
    fn open_session(
        &self,
        _: serde_json::Value,
    ) -> Result<String, codeterm_plugin::chatbackend_provider::ChatBackendError> {
        unreachable!()
    }
    fn send_message(
        &self,
        _: &str,
        _: &str,
    ) -> Result<(), codeterm_plugin::chatbackend_provider::ChatBackendError> {
        unreachable!()
    }
    fn poll(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<
        codeterm_types::agent::chat_types::StructuredChat,
        codeterm_plugin::chatbackend_provider::ChatBackendError,
    > {
        unreachable!()
    }
    fn close_session(
        &self,
        _: &str,
    ) -> Result<(), codeterm_plugin::chatbackend_provider::ChatBackendError> {
        unreachable!()
    }
    fn list_models(
        &self,
    ) -> Result<
        Vec<codeterm_plugin::provider::ModelInfo>,
        codeterm_plugin::chatbackend_provider::ChatBackendError,
    > {
        unreachable!()
    }
}

#[test]
fn blueprint_agent_cancel_and_expired_deadline_abort_the_provider() {
    for cancel in [true, false] {
        let (started, entered) = mpsc::channel();
        let (aborted, abort) = mpsc::channel();
        let provider = Arc::new(Fixture { started, aborted });
        let cancellation = codeterm_plugin::converter::ConversionCancellation::default();
        let request = AgentTaskRequest {
            input: codeterm_plugin::agent_task::AgentTaskInput {
                run_id: "fixture-run".into(),
                tab_id: "tab".into(),
                workspace_id: "workspace".into(),
                profile: "fixture".into(),
                task: "opaque".into(),
                input: String::new(),
                timeout_ms: 1000,
                max_tokens: 100,
                max_turns: 1,
            },
            deadline: if cancel {
                Instant::now() + Duration::from_secs(2)
            } else {
                Instant::now()
            },
            cancellation: cancellation.clone(),
        };
        let worker = std::thread::spawn(move || run(provider, request).unwrap());
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        if cancel {
            cancellation.cancel();
        }
        let result = worker.join().unwrap();
        assert_eq!(
            result.status,
            if cancel {
                AgentTaskStatus::Cancelled
            } else {
                AgentTaskStatus::DeadlineExceeded
            }
        );
        assert_eq!(
            abort.recv_timeout(Duration::from_secs(2)).unwrap(),
            "fixture-run"
        );
    }
}
