use super::{JsonRpcError, Value};

pub(super) fn task_files_field(
    params: Option<&Value>,
) -> Result<Option<codeterm_service_core::plan::TaskFiles>, JsonRpcError> {
    params
        .and_then(|p| p.get("files"))
        .map(|value| {
            serde_json::from_value::<codeterm_service_core::plan::TaskFiles>(value.clone())
                .map_err(|e| e.to_string())
                .and_then(codeterm_service_core::plan::TaskFiles::validated)
                .map_err(|message| JsonRpcError {
                    code: -32602,
                    message,
                    data: None,
                })
        })
        .transpose()
}
