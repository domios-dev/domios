use super::{
    engine,
    questions::{QuestionReceipt, QuestionRecord},
};
use crate::service::{
    chat_send_transaction::ChatSendStatus,
    pending_delivery_payloads::user_sends::runtime_questions::QuestionResolution, ServiceContext,
};
use codeterm_plugin::agent_runtime::*;

pub fn publish(
    svc: &ServiceContext,
    tab: &str,
    session: &engine::Session,
    questions: Vec<QuestionRecord>,
) -> Result<(), String> {
    engine::validate_binding(svc, session).map_err(|reason| reason.to_string())?;
    let value = serde_json::to_value(questions).map_err(|reason| reason.to_string())?;
    let nonce = session.context.lock().binding_nonce.clone();
    {
        let mut mux = svc.mux.write();
        let target = mux.get_tab_mut(tab).ok_or("Question tab unavailable")?;
        if target.session_binding_nonce.as_deref() != Some(&nonce) {
            return Err("Question binding changed".into());
        }
        target.agent_runtime_questions = Some(value.clone());
    }
    let state = session.state.lock().snapshot();
    svc.notify_event("agent-runtime-state", serde_json::json!({"tab_id":tab,"session_binding_nonce":nonce,"state":state,"questions":value}));
    svc.emit_state_changed();
    Ok(())
}

fn receipt(
    svc: &ServiceContext,
    record: &QuestionRecord,
) -> Result<Option<ChatSendStatus>, String> {
    record
        .resolved
        .as_ref()
        .map(|receipt| {
            let stored: ChatSendStatus = serde_json::from_value(receipt.status.clone())
                .map_err(|reason| reason.to_string())?;
            if stored.state == crate::service::chat_send_transaction::ChatSendState::Sent {
                return Ok(stored);
            }
            Ok(svc
                .chat_send_ledger
                .status(&receipt.send_id)
                .unwrap_or(stored))
        })
        .transpose()
}

pub(crate) fn answer(
    svc: &ServiceContext,
    tab: &str,
    item: &str,
    answers: Vec<RuntimeQuestionAnswer>,
) -> Result<ChatSendStatus, String> {
    if !super::requested_for_tab(svc, tab) {
        return Err("Tab has no agent runtime capability".into());
    }
    let thread = svc
        .mux
        .read()
        .get_tab(tab)
        .and_then(|target| target.agent_session_id.clone())
        .ok_or("Question thread unavailable")?;
    let store = &svc.pending_delivery_payloads.user_sends;
    svc.restore_durable_user_statuses(tab)?;
    let record = store
        .runtime_questions(tab, &thread)?
        .into_iter()
        .find(|record| record.item_id == item)
        .ok_or("Pending question unavailable")?;
    if let Some(status) = receipt(svc, &record)? {
        return Ok(status);
    }
    super::questions::validate_answers(&record.questions, &answers)
        .map_err(|reason| reason.to_string())?;
    let session = crate::async_runtime::block_on(engine::ensure(svc, tab))
        .map_err(|reason| reason.to_string())?;
    let input = crate::async_runtime::block_on(async {
        let _permit =
            tokio::time::timeout(std::time::Duration::from_secs(30), session.gate.acquire())
                .await
                .map_err(|_| "Question permit deadline")?
                .map_err(|_| "Runtime stopped")?;
        let context = session.context.lock().clone();
        if context.thread_id.as_deref() != Some(&thread) {
            return Err("Question thread changed".to_string());
        }
        let operation = RuntimeOperation::AnswerQuestion {
            thread_id: thread.clone(),
            item_id: item.into(),
            answers: answers.clone(),
        };
        let adapter = session
            .adapter
            .lock()
            .clone()
            .ok_or("Runtime provider unavailable")?;
        let capability = adapter
            .agent_runtime()
            .ok_or("Runtime capability unavailable")?;
        validate_runtime_operation(&context, &operation).map_err(|reason| reason.to_string())?;
        let plan = engine::plan(&session, &context, &operation)
            .await
            .map_err(|reason| reason.to_string())?;
        validate_runtime_plan(&capability, &context, &operation, &plan)
            .map_err(|reason| reason.to_string())?;
        engine::validate_binding(svc, &session).map_err(|reason| reason.to_string())?;
        if session.context.lock().epoch != context.epoch {
            return Err("Question epoch changed".into());
        }
        let RuntimeAction::QuestionAnswer { input } = plan.action else {
            return Err("Question hook must format a user turn".into());
        };
        Ok(input)
    })?;
    let reservation =
        svc.reserve_user_send(tab, Some(format!("runtime-question:{thread}:{item}")))?;
    let status = svc.chat_send_ledger.reserved_user_status(&reservation);
    let resolution = QuestionResolution {
        thread_id: thread.clone(),
        item_id: item.into(),
        receipt: QuestionReceipt {
            answers,
            send_id: reservation.send_id.clone(),
            status: serde_json::to_value(&status).map_err(|reason| reason.to_string())?,
        },
    };
    let result = svc
        .gateway
        .submit_runtime_question_answer(svc, reservation, input, resolution);
    let questions = store.runtime_questions(tab, &thread)?;
    publish(svc, tab, &session, questions.clone())?;
    if let Err(error) = result {
        if let Some(status) = questions
            .iter()
            .find(|record| record.item_id == item)
            .map(|record| receipt(svc, record))
            .transpose()?
            .flatten()
        {
            return Ok(status);
        }
        return Err(error);
    }
    result
}
