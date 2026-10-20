use super::ServiceContext;

impl ServiceContext {
    pub fn configuration(&self) -> crate::config::schema::AppConfig {
        self.ext::<crate::config::ConfigState>().lock().clone()
    }
}
