use super::*;
use tokio::sync::mpsc;

static PORTS: Mutex<()> = Mutex::const_new(());

struct BrowserProbe {
    opened: mpsc::UnboundedSender<url::Url>,
    outcome: Option<Result<()>>,
}

#[async_trait]
impl Browser for BrowserProbe {
    async fn open(&self, url: &url::Url) -> Result<()> {
        self.opened.send(url.clone()).unwrap();
        match self.outcome {
            Some(outcome) => outcome,
            None => std::future::pending().await,
        }
    }
}

fn account() -> Arc<Account> {
    Arc::new(Account::new(
        Arc::new(adapters::fake::Fake::default()),
        Arc::new(super::sign_in_tests::Store::default()),
        Duration::from_secs(180),
    ))
}

async fn start(
    account: &Arc<Account>,
    outcome: Option<Result<()>>,
) -> (
    String,
    tokio::task::JoinHandle<LoginCompletion>,
    mpsc::UnboundedReceiver<url::Url>,
) {
    let (opened, receiver) = mpsc::unbounded_channel();
    let (id, task) = account
        .start_login(
            Arc::new(BrowserProbe { opened, outcome }),
            LoginHint::default(),
            |_, _| {},
        )
        .await
        .unwrap();
    (id, task, receiver)
}

fn parameter(url: &url::Url, name: &str) -> String {
    url.query_pairs()
        .find(|(key, _)| key == name)
        .unwrap()
        .1
        .into_owned()
}

fn address(url: &url::Url) -> std::net::SocketAddr {
    let redirect = url::Url::parse(&parameter(url, "redirect_uri")).unwrap();
    format!("127.0.0.1:{}", redirect.port().unwrap())
        .parse()
        .unwrap()
}

fn assert_port_free(url: &url::Url) {
    let listener = std::net::TcpListener::bind(address(url))
        .expect("Finished login must release its callback listener");
    drop(listener);
}

#[tokio::test(start_paused = true)]
async fn callback_listener_timeout_releases_port() {
    let _ports = PORTS.lock().await;
    let account = account();
    let (_, login, mut opened) = start(&account, Some(Ok(()))).await;
    let url = opened.recv().await.unwrap();
    tokio::time::advance(Duration::from_secs(180)).await;
    assert_eq!(login.await.unwrap().0.unwrap_err(), Error::Timeout);
    assert_port_free(&url);
    assert!(!account.status().await.pending);
}

#[tokio::test(start_paused = true)]
async fn callback_listener_logout_cancels_pending_browser_and_releases_port() {
    let _ports = PORTS.lock().await;
    for browser_outcome in [None, Some(Ok(()))] {
        let account = account();
        let (_, login, mut opened) = start(&account, browser_outcome).await;
        let url = opened.recv().await.unwrap();
        account.logout().await.unwrap();
        assert_eq!(login.await.unwrap().0.unwrap_err(), Error::Cancelled);
        assert_port_free(&url);
    }
}

#[tokio::test]
async fn callback_listener_cancel_matches_only_the_attempt_and_preserves_identity() {
    let _ports = PORTS.lock().await;
    let account = account();
    let (prior_id, first, mut opened) = start(&account, Some(Ok(()))).await;
    let url = opened.recv().await.unwrap();
    complete_http(&url).await;
    let prior = first.await.unwrap().0.unwrap().identity.unwrap();

    let (id, replacement, mut opened) = start(&account, Some(Ok(()))).await;
    let url = opened.recv().await.unwrap();
    assert!(!account.cancel_login(&prior_id).await);
    assert!(account.status().await.pending);
    assert!(
        account.operation.try_lock().is_ok(),
        "callback must not own the account operation lock"
    );
    assert!(account.cancel_login(&id).await);
    assert!(!account.cancel_login(&id).await);
    assert_eq!(replacement.await.unwrap().0.unwrap_err(), Error::Cancelled);
    assert_port_free(&url);
    let status = account.status().await;
    assert!(!status.pending);
    assert_eq!(status.identity, Some(prior));

    let (next, next_task, mut opened) = start(&account, Some(Ok(()))).await;
    let next_url = opened.recv().await.unwrap();
    assert_ne!(id, next);
    assert!(!account.cancel_login(&id).await);
    assert_eq!(
        account
            .complete(Callback {
                code: "c".repeat(43),
                state: parameter(&url, "state")
            })
            .await,
        Err(Error::WrongState),
    );
    complete_http(&next_url).await;
    assert!(next_task.await.unwrap().0.unwrap().identity.is_some());
}

#[tokio::test]
async fn callback_listener_browser_failure_releases_port() {
    let _ports = PORTS.lock().await;
    let account = account();
    let (_, login, mut opened) = start(&account, Some(Err(Error::Transport))).await;
    let url = opened.recv().await.unwrap();
    assert_eq!(login.await.unwrap().0.unwrap_err(), Error::Transport);
    assert_port_free(&url);
}

#[tokio::test]
async fn callback_listener_refuses_second_start_while_first_is_pending() {
    let _ports = PORTS.lock().await;
    let account = account();
    let (id, login, mut opened) = start(&account, Some(Ok(()))).await;
    let url = opened.recv().await.unwrap();
    let (sender, _) = mpsc::unbounded_channel();
    let duplicate = account
        .start_login(
            Arc::new(BrowserProbe {
                opened: sender,
                outcome: None,
            }),
            LoginHint::default(),
            |_, _| {},
        )
        .await;
    assert!(matches!(duplicate, Err(Error::Pending)));
    assert_eq!(account.status_with_login().await.1, Some(id));
    complete_http(&url).await;
    assert!(login.await.unwrap().0.unwrap().identity.is_some());
    assert_port_free(&url);
}

async fn complete_http(url: &url::Url) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let address = address(url);
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let request = format!(
        "GET /callback?code={}&state={} HTTP/1.1\r\nHost: {address}\r\n\r\n",
        "c".repeat(43),
        parameter(url, "state"),
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"));
}

#[tokio::test]
async fn callback_listener_http_success_releases_port() {
    let _ports = PORTS.lock().await;
    let account = account();
    let (_, login, mut opened) = start(&account, Some(Ok(()))).await;
    let url = opened.recv().await.unwrap();
    complete_http(&url).await;
    assert!(login.await.unwrap().0.unwrap().identity.is_some());
    assert_port_free(&url);
}
