use std::path::PathBuf;

use serde_json::Value;

pub trait UiEvents: Send + Sync {
    fn emit(&self, event: &str, payload: Value);
    fn emit_native(&self, event: &str, payload: Value) {
        self.emit(event, payload);
    }
    fn emit_to(&self, target: &str, event: &str, payload: Value);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellError {
    Unavailable,
    Failed(String),
}

impl std::fmt::Display for ShellError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("shell is unavailable"),
            Self::Failed(error) => formatter.write_str(error),
        }
    }
}

impl std::error::Error for ShellError {}

pub struct WebviewRequest {
    pub label: String,
    pub url: String,
    pub title: String,
    pub width: f64,
    pub height: f64,
}

/// Window operations must run off the main thread; host adapters may wait for it.
pub trait Shell: Send + Sync {
    fn focus_window(&self, label: &str) -> Result<(), ShellError>;
    fn window_exists(&self, label: &str) -> bool;
    fn os_notify(&self, title: &str, body: Option<&str>) -> Result<(), ShellError>;
    fn toast_permission(&self, enabling: Option<bool>) -> String;
    fn open_webview(&self, request: WebviewRequest) -> Result<(), ShellError>;
    fn current_exe(&self) -> Result<PathBuf, ShellError>;
    fn config_dir(&self) -> Result<PathBuf, ShellError>;
    fn resource_dir(&self) -> Result<PathBuf, ShellError>;
}

pub struct NoShell;

impl Shell for NoShell {
    fn resource_dir(&self) -> Result<PathBuf, ShellError> {
        Err(ShellError::Unavailable)
    }

    fn focus_window(&self, _: &str) -> Result<(), ShellError> {
        Err(ShellError::Unavailable)
    }

    fn window_exists(&self, _: &str) -> bool {
        false
    }

    fn os_notify(&self, _: &str, _: Option<&str>) -> Result<(), ShellError> {
        Err(ShellError::Unavailable)
    }

    fn toast_permission(&self, _: Option<bool>) -> String {
        "unknown".to_string()
    }

    fn open_webview(&self, _: WebviewRequest) -> Result<(), ShellError> {
        Err(ShellError::Unavailable)
    }

    fn current_exe(&self) -> Result<PathBuf, ShellError> {
        Err(ShellError::Unavailable)
    }

    fn config_dir(&self) -> Result<PathBuf, ShellError> {
        Err(ShellError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn host_ports_are_object_safe() {
        super::super::assert_port::<dyn super::UiEvents>();
        super::super::assert_port::<dyn super::Shell>();
    }
}
