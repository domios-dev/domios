use crate::agent_runtime::*;
use crate::provider::AgentProvider;
use codeterm_types::agent::chat_types::ChatMsg;
use serde_json::{json, Value};
use std::path::PathBuf;

pub(super) struct NativeRuntime(pub AgentRuntimeCapability);

#[async_trait::async_trait]
impl AgentProvider for NativeRuntime {
    fn id(&self) -> &str {
        "echo-runtime"
    }
    fn display_name(&self) -> &str {
        "Fixture"
    }
    fn build_launch_command(&self, _: &crate::provider::AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> crate::provider::ReadinessStrategy {
        crate::provider::ReadinessStrategy::PollShellPrompt {
            timeout: std::time::Duration::from_secs(1),
        }
    }
    fn task_delivery(&self) -> crate::provider::TaskDelivery {
        crate::provider::TaskDelivery::PtyInput
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_session_id(
        &self,
        _: &str,
        _: &[String],
        _: Option<std::time::Duration>,
    ) -> Option<String> {
        None
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn agent_runtime(&self) -> Option<AgentRuntimeCapability> {
        Some(self.0.clone())
    }

    async fn runtime_plan(
        &self,
        ctx: &RuntimeContext,
        op: &RuntimeOperation,
    ) -> Result<RuntimePlan, RuntimeFailure> {
        validate_runtime_operation(ctx, op)?;
        let action = match op {
            RuntimeOperation::AnswerQuestion { answers, .. } => RuntimeAction::QuestionAnswer {
                input: TurnInput {
                    text: answers
                        .iter()
                        .flat_map(|answer| answer.values.iter().cloned())
                        .collect::<Vec<_>>()
                        .join("\n"),
                    attachments: vec![],
                },
            },
            RuntimeOperation::Interrupt {
                thread_id,
                active_turn_id,
            } => RuntimeAction::Request {
                call: RpcCall {
                    target: Some(RuntimeCallTarget {
                        thread_id: thread_id.clone(),
                        turn_id: active_turn_id.clone(),
                    }),
                    method: op.kind().into(),
                    params: json!(op),
                },
            },
            RuntimeOperation::Answer { request_id, answer } => RuntimeAction::Reply {
                request_id: request_id.clone(),
                result: RpcReply::Success {
                    value: json!(answer),
                },
            },
            _ => RuntimeAction::Request {
                call: RpcCall {
                    target: None,
                    method: op.kind().into(),
                    params: json!(op),
                },
            },
        };
        let plan = RuntimePlan {
            action,
            state: Value::Null,
        };
        validate_runtime_plan(&self.0, ctx, op, &plan)?;
        Ok(plan)
    }

    async fn runtime_response(
        &self,
        ctx: &RuntimeContext,
        op: &RuntimeOperation,
        state: &Value,
        reply: &RpcReply,
    ) -> Result<RuntimeStep, RuntimeFailure> {
        validate_runtime_operation(ctx, op)?;
        validate_runtime_state(state)?;
        let step = match reply {
            RpcReply::Error { error } => RuntimeStep::Refused {
                reason: RuntimeFailure::new(RuntimeFailureCode::ProviderRefused, &error.message),
                before_acceptance: true,
            },
            RpcReply::Success { value } => {
                let result = match op {
                    RuntimeOperation::Initialize => RuntimeResult::Initialized,
                    RuntimeOperation::OpenThread { settings, .. } => RuntimeResult::ThreadLoaded {
                        thread_id: value["threadId"]
                            .as_str()
                            .ok_or_else(|| {
                                RuntimeFailure::new(
                                    RuntimeFailureCode::InvalidSchema,
                                    "missing thread",
                                )
                            })?
                            .into(),
                        settings: settings.clone(),
                    },
                    RuntimeOperation::ResumeThread {
                        thread_id,
                        settings,
                        ..
                    } => RuntimeResult::ThreadLoaded {
                        thread_id: thread_id.clone(),
                        settings: settings.clone(),
                    },
                    RuntimeOperation::ReadHistory { thread_id, .. } => RuntimeResult::HistoryPage {
                        thread_id: thread_id.clone(),
                        items: serde_json::from_value(value["items"].clone()).map_err(|e| {
                            RuntimeFailure::new(RuntimeFailureCode::InvalidSchema, e.to_string())
                        })?,
                        next_cursor: None,
                        open_items: Vec::new(),
                        questions: None,
                    },
                    RuntimeOperation::SubmitTurn {
                        thread_id, send_id, ..
                    } => RuntimeResult::TurnAccepted {
                        thread_id: thread_id.clone(),
                        send_id: send_id.clone(),
                        turn_id: None,
                    },
                    _ => RuntimeResult::Done,
                };
                if value["more"] == true && matches!(op, RuntimeOperation::ReadHistory { .. }) {
                    RuntimeStep::Continue {
                        history: Some(result),
                        plan: RuntimePlan {
                            action: RuntimeAction::Request {
                                call: RpcCall {
                                    method: op.kind().into(),
                                    params: json!(op),
                                    target: None,
                                },
                            },
                            state: Value::Null,
                        },
                        before_acceptance: false,
                    }
                } else {
                    RuntimeStep::Complete {
                        result,
                        events: None,
                        notification: None,
                    }
                }
            }
        };
        validate_runtime_step(&self.0, ctx, op, reply, &step)?;
        Ok(step)
    }

    async fn runtime_reduce(
        &self,
        ctx: &RuntimeContext,
        state: &Value,
        frames: &[RuntimeFrame],
    ) -> Result<RuntimeReduction, RuntimeFailure> {
        validate_runtime_context(ctx)?;
        validate_runtime_state(state)?;
        validate_runtime_frames(&self.0, frames)?;
        let thread = ctx
            .thread_id
            .as_ref()
            .ok_or_else(|| RuntimeFailure::new(RuntimeFailureCode::BindingMismatch, "no thread"))?;
        let mut events = Vec::new();
        for frame in frames {
            let params = match frame {
                RuntimeFrame::Request { params, .. }
                | RuntimeFrame::Notification { params, .. } => params,
            };
            let item: ChatMsg = serde_json::from_value(params.clone()).map_err(|e| {
                RuntimeFailure::new(RuntimeFailureCode::InvalidSchema, e.to_string())
            })?;
            events.push(AgentRuntimeEvent::ChatItem {
                thread_id: thread.clone(),
                item,
            });
        }
        let reduction = RuntimeReduction {
            state: json!({"count":state["count"].as_u64().unwrap_or(0)+frames.len() as u64}),
            events,
        };
        validate_runtime_reduction(ctx, &reduction)?;
        Ok(reduction)
    }
}

pub(super) fn native_frame_baseline(state: &Value, frames: &[RuntimeFrame]) -> RuntimeReduction {
    let mapped: Vec<Value> = frames
        .iter()
        .map(|frame| match frame {
            RuntimeFrame::Notification { method, params } => {
                json!({"method":method,"params":params})
            }
            RuntimeFrame::Request {
                request_id,
                method,
                params,
            } => json!({"requestId":request_id,"method":method,"params":params}),
        })
        .collect();
    RuntimeReduction {
        state: json!({"count":state["count"].as_u64().unwrap_or(0)+frames.len() as u64,"frames":mapped}),
        events: Vec::new(),
    }
}
