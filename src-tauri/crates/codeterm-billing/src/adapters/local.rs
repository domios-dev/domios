use crate::*;
use codeterm_account::Session;
pub struct Local {
    pub read: Box<dyn Fn() -> Result<Entitlements> + Send + Sync>,
    pub origin: url::Url,
}
impl Local {
    fn route(&self, path: &str) -> Result<url::Url> {
        self.origin
            .join(path)
            .map_err(|_| "Invalid local billing URL".into())
    }
}
#[async_trait]
impl BillingProvider for Local {
    async fn entitlements(&self, _: &Session) -> Result<Entitlements> {
        (self.read)()
    }
    async fn checkout_url(&self, _: &Session, plan: &str) -> Result<url::Url> {
        let mut url = self.route("billing/checkout")?;
        url.query_pairs_mut().append_pair("plan", plan);
        Ok(url)
    }
    async fn portal_url(&self, _: &Session) -> Result<url::Url> {
        self.route("billing/portal")
    }
}
