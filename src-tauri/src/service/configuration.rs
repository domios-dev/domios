use super::ServiceContext;
use tauri::Manager;

impl ServiceContext {
    pub(crate) fn configuration(&self) -> crate::config::schema::AppConfig {
        self.app_handle
            .state::<crate::config::ConfigState>()
            .lock()
            .clone()
    }
}
