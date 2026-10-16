use std::cell::RefCell;

#[derive(Clone, Debug, PartialEq)]
pub struct RequestArrival {
    pub request_id: Option<serde_json::Value>,
    pub method: String,
    pub received_at_us: u128,
    pub dispatched_at_us: u128,
}

thread_local! {
    static REQUEST_ARRIVAL: RefCell<Option<RequestArrival>> = const { RefCell::new(None) };
}

pub struct RequestArrivalGuard {
    previous: Option<RequestArrival>,
    _thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl Drop for RequestArrivalGuard {
    fn drop(&mut self) {
        REQUEST_ARRIVAL.with(|arrival| arrival.replace(self.previous.take()));
    }
}

pub fn timestamp_us() -> u128 {
    if log::STATIC_MAX_LEVEL < log::LevelFilter::Debug {
        return 0;
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros()
}

pub fn enter_request(
    request_id: Option<&serde_json::Value>,
    method: &str,
    received_at_us: u128,
) -> RequestArrivalGuard {
    let context = (log::STATIC_MAX_LEVEL >= log::LevelFilter::Debug).then(|| RequestArrival {
        request_id: request_id.cloned(),
        method: method.to_owned(),
        received_at_us,
        dispatched_at_us: timestamp_us(),
    });
    RequestArrivalGuard {
        previous: REQUEST_ARRIVAL.with(|arrival| arrival.replace(context)),
        _thread_bound: std::marker::PhantomData,
    }
}

fn phase_line(tab_id: &str, phase: &str, at_us: u128) -> String {
    format!("[boot-phase] tab={tab_id} pty=- phase={phase:?} at_us={at_us}")
}

fn request_lines(
    tab_id: &str,
    arrival: Option<&RequestArrival>,
    bound_at_us: u128,
) -> Option<[String; 3]> {
    let arrival = arrival?;
    Some([
        phase_line(tab_id, "request_received", arrival.received_at_us),
        phase_line(tab_id, "request_dispatched", arrival.dispatched_at_us),
        format!(
            "[boot-request] tab={} request_id={} method={} received_at_us={} dispatched_at_us={} bound_at_us={}",
            tab_id,
            arrival.request_id.as_ref().unwrap_or(&serde_json::Value::Null),
            arrival.method,
            arrival.received_at_us,
            arrival.dispatched_at_us,
            bound_at_us,
        ),
    ])
}

pub fn bind_request(tab_id: &str) {
    if log::STATIC_MAX_LEVEL < log::LevelFilter::Debug {
        return;
    }
    REQUEST_ARRIVAL.with(|arrival| {
        if let Some(lines) = request_lines(tab_id, arrival.borrow().as_ref(), timestamp_us()) {
            for line in lines {
                log::debug!("{line}");
            }
        }
    });
}

pub fn mark(tab_id: &str, pty_id: Option<&str>, phase: impl std::fmt::Debug) {
    log::debug!(
        "[boot-phase] tab={} pty={} phase={:?} at_us={}",
        tab_id,
        pty_id.unwrap_or("-"),
        phase,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_formatter_matches_boot_phase_grammar() {
        let arrival = RequestArrival {
            request_id: Some(serde_json::json!("request-7")),
            method: "agent.spawn".into(),
            received_at_us: 101,
            dispatched_at_us: 202,
        };
        assert_eq!(request_lines("tab-3", Some(&arrival), 303).unwrap(), [
            "[boot-phase] tab=tab-3 pty=- phase=\"request_received\" at_us=101",
            "[boot-phase] tab=tab-3 pty=- phase=\"request_dispatched\" at_us=202",
            "[boot-request] tab=tab-3 request_id=\"request-7\" method=agent.spawn received_at_us=101 dispatched_at_us=202 bound_at_us=303",
        ]);
    }

    #[test]
    fn request_guard_clears_after_return_and_panic() {
        for panic in [false, true] {
            let result = std::panic::catch_unwind(|| {
                let _guard = enter_request(Some(&serde_json::json!(7)), "agent.spawn", 101);
                REQUEST_ARRIVAL.with(|arrival| assert!(arrival.borrow().is_some()));
                assert!(!panic, "dispatch panic");
            });
            assert_eq!(result.is_err(), panic);
            REQUEST_ARRIVAL.with(|arrival| assert!(arrival.borrow().is_none()));
        }
    }

    #[test]
    fn request_guard_restores_nested_context() {
        let _outer = enter_request(None, "outer", 101);
        {
            let _inner = enter_request(None, "inner", 202);
        }
        REQUEST_ARRIVAL.with(|arrival| {
            assert_eq!(arrival.borrow().as_ref().unwrap().method, "outer");
        });
    }

    #[test]
    fn bind_request_without_context_emits_nothing() {
        bind_request("internal-tab");
        REQUEST_ARRIVAL.with(|arrival| {
            assert_eq!(
                request_lines("internal-tab", arrival.borrow().as_ref(), 303),
                None
            );
        });
    }
}
