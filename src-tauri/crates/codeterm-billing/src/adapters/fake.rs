use crate::*;
use codeterm_account::Session;
use parking_lot::Mutex;
pub struct Fake {
    pub value: Mutex<Result<Entitlements>>,
    pub calls: Mutex<usize>,
}
impl Default for Fake {
    fn default() -> Self {
        Self {
            value: Mutex::new(Ok(Entitlements::default())),
            calls: Mutex::new(0),
        }
    }
}
#[async_trait]
impl BillingProvider for Fake {
    async fn entitlements(&self, _: &Session) -> Result<Entitlements> {
        *self.calls.lock() += 1;
        self.value.lock().clone()
    }
    async fn checkout_url(&self, _: &Session, _: &str) -> Result<url::Url> {
        url::Url::parse("https://billing.example/checkout").map_err(|e| e.to_string())
    }
    async fn portal_url(&self, _: &Session) -> Result<url::Url> {
        url::Url::parse("https://billing.example/portal").map_err(|e| e.to_string())
    }
}
