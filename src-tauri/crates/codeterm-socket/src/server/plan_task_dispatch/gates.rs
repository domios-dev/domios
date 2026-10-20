use serde_json::Value;

use super::super::JsonRpcError;
use super::lane_resolution::invalid_dispatch;

const MAX_AUTHORIZED_GATES: usize = 32;
const MAX_AUTHORIZED_GATE_CHARS: usize = 4096;

pub(super) fn dispatch_authorized_gates(p: Option<&Value>) -> Result<Vec<String>, JsonRpcError> {
    let Some(value) = p.and_then(|params| params.get("authorized_gates")) else {
        return Ok(Vec::new());
    };
    let Some(values) = value.as_array() else {
        return Err(invalid_dispatch("authorized_gates must be an array"));
    };
    if values.len() > MAX_AUTHORIZED_GATES {
        return Err(invalid_dispatch(format!(
            "authorized_gates may contain at most {MAX_AUTHORIZED_GATES} values"
        )));
    }
    values
        .iter()
        .map(|value| {
            let gate = value
                .as_str()
                .ok_or_else(|| invalid_dispatch("authorized_gates values must be strings"))?;
            if gate.is_empty() {
                return Err(invalid_dispatch("authorized_gates values must not be empty"));
            }
            if gate.chars().count() > MAX_AUTHORIZED_GATE_CHARS {
                return Err(invalid_dispatch(format!(
                    "authorized_gates values may contain at most {MAX_AUTHORIZED_GATE_CHARS} characters"
                )));
            }
            if gate.chars().any(char::is_control) {
                return Err(invalid_dispatch(
                    "authorized_gates values must not contain control characters",
                ));
            }
            Ok(gate.to_string())
        })
        .collect()
}
