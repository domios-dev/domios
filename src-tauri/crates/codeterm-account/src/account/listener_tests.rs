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

fn start(
    account: &Arc<Account>,
    outcome: Option<Result<()>>,
) -> (
    tokio::task::JoinHandle<Result<Status>>,
    mpsc::UnboundedReceiver<url::Url>,
) {
    let (opened, receiver) = mpsc::unbounded_channel();
    let account = account.clone();
    let login = tokio::spawn(async move { account.login(&BrowserProbe { opened, outcome }).await });
    (login, receiver)
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
    let (login, mut opened) = start(&account, Some(Ok(())));
    let url = opened.recv().await.unwrap();
    tokio::time::advance(Duration::from_secs(180)).await;
    assert_eq!(login.await.unwrap().unwrap_err(), Error::Timeout);
    assert_port_free(&url);
    assert!(!account.status().await.pending);
}

#[tokio::test(start_paused = true)]
async fn callback_listener_logout_cancels_pending_browser_and_releases_port() {
    let _ports = PORTS.lock().await;
    for browser_outcome in [None, Some(Ok(()))] {
        let account = account();
        let (login, mut opened) = start(&account, browser_outcome);
        let url = opened.recv().await.unwrap();
        account.logout().await.unwrap();
        assert_eq!(login.await.unwrap().unwrap_err(), Error::Cancelled);
        assert_port_free(&url);
    }
}

#[tokio::test]
async fn callback_listener_supersede_closes_old_port_before_opening_replacement() {
    let _ports = PORTS.lock().await;
    let account = account();
    let (old, mut opened) = start(&account, None);
    let old_url = opened.recv().await.unwrap();
    let (replacement, mut opened) = start(&account, Some(Ok(())));
    let new_url = opened.recv().await.unwrap();
    assert_eq!(old.await.unwrap().unwrap_err(), Error::Cancelled);
    assert_port_free(&old_url);
    assert!(account.status().await.pending);
    account
        .complete(Callback {
            code: "c".repeat(43),
            state: parameter(&new_url, "state"),
        })
        .await
        .unwrap();
    assert!(replacement.await.unwrap().unwrap().identity.is_some());
    assert_port_free(&new_url);
}

#[tokio::test]
async fn callback_listener_browser_failure_releases_port() {
    let _ports = PORTS.lock().await;
    let account = account();
    let (login, mut opened) = start(&account, Some(Err(Error::Transport)));
    let url = opened.recv().await.unwrap();
    assert_eq!(login.await.unwrap().unwrap_err(), Error::Transport);
    assert_port_free(&url);
}

#[tokio::test]
async fn callback_listener_caller_abort_releases_port() {
    let _ports = PORTS.lock().await;
    let account = account();
    let (login, mut opened) = start(&account, Some(Ok(())));
    let url = opened.recv().await.unwrap();
    login.abort();
    assert!(login.await.unwrap_err().is_cancelled());
    assert_port_free(&url);
}

#[tokio::test]
async fn callback_listener_http_success_releases_port() {
    let _ports = PORTS.lock().await;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let account = account();
    let (login, mut opened) = start(&account, Some(Ok(())));
    let url = opened.recv().await.unwrap();
    let address = address(&url);
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let request = format!(
        "GET /callback?code={}&state={} HTTP/1.1\r\nHost: {address}\r\n\r\n",
        "c".repeat(43),
        parameter(&url, "state"),
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(login.await.unwrap().unwrap().identity.is_some());
    assert_port_free(&url);
}
