use super::*;
use codeterm_plugin::agent_runtime::RuntimeHttpRequestEvent;
use std::collections::VecDeque;

#[derive(Clone)]
struct Route {
    event: RuntimeHttpRequestEvent,
    path: Value,
}

#[derive(Default)]
pub(super) struct Requests {
    pending: HashMap<RpcId, Route>,
    seen: VecDeque<RpcId>,
}

impl Requests {
    pub fn frame(
        &mut self,
        client: &Client,
        declaration: &RuntimeHttpSse,
        params: Value,
    ) -> Result<Option<RuntimeFrame>, RuntimeFailure> {
        for (method, event) in &declaration.request_events {
            if params.pointer(&event.type_pointer).and_then(Value::as_str) != Some(method) {
                continue;
            }
            let thread = client.0.scoped_thread.lock().clone();
            if thread.as_deref().is_none()
                || params
                    .pointer(&event.thread_pointer)
                    .and_then(Value::as_str)
                    != thread.as_deref()
            {
                return Ok(None);
            }
            let id = params
                .pointer(&event.id_pointer)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty() && value.len() <= 4096)
                .ok_or_else(|| failure("SSE request id invalid"))?;
            let id = RpcId::String(id.into());
            if self.pending.contains_key(&id) || self.seen.contains(&id) {
                return Ok(None);
            }
            if self.pending.len() >= 128 {
                return Err(failure("SSE pending request limit"));
            }
            let mut path = serde_json::Map::new();
            for (name, pointer) in &event.path_params {
                let value = params
                    .pointer(pointer)
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty() && value.len() <= 4096)
                    .ok_or_else(|| failure("SSE request route invalid"))?;
                path.insert(name.clone(), json!(value));
            }
            self.pending.insert(
                id.clone(),
                Route {
                    event: event.clone(),
                    path: Value::Object(path),
                },
            );
            return Ok(Some(RuntimeFrame::Request {
                request_id: id,
                method: method.clone(),
                params,
            }));
        }
        Ok(Some(RuntimeFrame::Notification {
            method: declaration.event_method.clone(),
            params,
        }))
    }

    pub fn reply(&self, frame: &Value) -> Result<(RpcId, Value), RuntimeFailure> {
        let id: RpcId = serde_json::from_value(frame["id"].clone())
            .map_err(|_| failure("HTTP reply id invalid"))?;
        let route = self.pending.get(&id).ok_or_else(|| {
            RuntimeFailure::PrewriteRefused("HTTP request no longer pending".into())
        })?;
        let rejected = frame.get("error").is_some();
        let method = if rejected {
            &route.event.reject_method
        } else {
            &route.event.reply_method
        };
        let body = if rejected {
            &route.event.reject_body
        } else {
            &frame["result"]
        };
        Ok((
            id,
            json!({"method":method,"params":{"path":route.path,"body":body}}),
        ))
    }

    pub fn claim(&mut self, id: &RpcId) -> Result<(), RuntimeFailure> {
        if self.pending.remove(id).is_none() {
            return Err(RuntimeFailure::PrewriteRefused(
                "HTTP reply already claimed".into(),
            ));
        }
        self.seen.push_back(id.clone());
        if self.seen.len() > 1024 {
            self.seen.pop_front();
        }
        Ok(())
    }
}
