use super::*;
use tokio::sync::{oneshot, Mutex};

type LoginResult = tokio::task::JoinHandle<Result<codeterm_account::Status, String>>;

#[derive(Default)]
struct PasteLoginCell(Mutex<Option<PasteLogin>>);

struct PasteLogin {
    id: String,
    url: url::Url,
    task: LoginResult,
}

struct CaptureBrowser(Mutex<Option<oneshot::Sender<url::Url>>>);

#[async_trait]
impl Browser for CaptureBrowser {
    async fn open(&self, url: &url::Url) -> codeterm_account::Result<()> {
        self.0
            .lock()
            .await
            .take()
            .ok_or(Error::Cancelled)?
            .send(url.clone())
            .map_err(|_| Error::Cancelled)
    }
}

impl crate::service::ServiceContext {
    pub async fn account_login_paste_start(&self) -> Result<(String, url::Url), String> {
        let cell = self
            .extensions
            .get_or_init::<PasteLoginCell>(Default::default);
        let mut pending = cell.0.lock().await;
        let (sender, receiver) = oneshot::channel();
        let browser = Arc::new(CaptureBrowser(Mutex::new(Some(sender))));
        let service = self.clone();
        let mut task = tokio::spawn(async move {
            service
                .finish_account_login_with_browser(Default::default(), browser)
                .await
        });
        let url = tokio::select! {
            url = receiver => url.map_err(|_| "Sign-in did not produce an authorization URL".to_owned())?,
            result = &mut task => return Err(result.map_err(|_| "Sign-in was cancelled".to_owned())?.err().unwrap_or_else(|| "Provider does not support browser sign-in".into())),
        };
        let id = uuid::Uuid::new_v4().to_string();
        *pending = Some(PasteLogin {
            id: id.clone(),
            url: url.clone(),
            task,
        });
        Ok((id, url))
    }

    pub async fn account_login_paste_pending(&self) -> Option<(String, url::Url)> {
        let cell = self
            .extensions
            .get_or_init::<PasteLoginCell>(Default::default);
        let mut pending = cell.0.lock().await;
        if pending
            .as_ref()
            .is_some_and(|login| login.task.is_finished())
        {
            *pending = None;
        }
        pending
            .as_ref()
            .map(|login| (login.id.clone(), login.url.clone()))
    }

    pub async fn account_login_paste_complete(
        &self,
        id: &str,
        callback: codeterm_account::Callback,
    ) -> Result<codeterm_account::Status, String> {
        let cell = self
            .extensions
            .get_or_init::<PasteLoginCell>(Default::default);
        let mut pending = cell.0.lock().await;
        if !pending.as_ref().is_some_and(|login| login.id == id) {
            return Err("This paste-back sign-in is no longer pending".into());
        }
        self.account()
            .await?
            .complete(callback)
            .await
            .map_err(|e| e.to_string())?;
        let login = pending
            .take()
            .ok_or_else(|| "Sign-in is no longer pending".to_owned())?;
        login
            .task
            .await
            .map_err(|_| "Sign-in was cancelled".to_owned())?
    }
}

#[cfg(test)]
mod tests;
