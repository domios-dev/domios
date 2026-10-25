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
    let login = tokio::spawn(async move {
        account
            .login(Arc::new(BrowserProbe { opened, outcome }))
            .await
    });
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
async fn callback_listener_caller_abort_survives_until_callback() {
    let _ports = PORTS.lock().await;
    let account = account();
    let (login, mut opened) = start(&account, Some(Ok(())));
    let url = opened.recv().await.unwrap();
    login.abort();
    assert!(login.await.unwrap_err().is_cancelled());
    assert!(account.status().await.pending);
    complete_http(&url).await;
    let _finished = account.operation.lock().await;
    let status = account.status().await;
    assert!(!status.pending);
    assert_eq!(status.identity.unwrap().uid, "fake-user");
    assert!(status.last_sign_in_failure.is_none());
    assert_eq!(
        account
            .complete(Callback {
                code: "c".repeat(43),
                state: parameter(&url, "state")
            })
            .await,
        Err(Error::Consumed)
    );
    assert_port_free(&url);
}

#[tokio::test(start_paused = true)]
async fn callback_listener_caller_abort_still_times_out() {
    let _ports = PORTS.lock().await;
    for outcome in [None, Some(Ok(()))] {
        let account = account();
        let (login, mut opened) = start(&account, outcome);
        let url = opened.recv().await.unwrap();
        login.abort();
        assert!(login.await.unwrap_err().is_cancelled());
        tokio::time::advance(Duration::from_secs(180)).await;
        let _finished = account.operation.lock().await;
        let status = account.status().await;
        assert!(!status.pending);
        assert!(status.identity.is_none());
        assert_eq!(status.last_sign_in_failure.unwrap().error, Error::Timeout);
        assert_port_free(&url);
        assert_eq!(
            account
                .complete(Callback {
                    code: "c".repeat(43),
                    state: parameter(&url, "state")
                })
                .await,
            Err(Error::Consumed)
        );
    }
}

#[tokio::test]
async fn callback_listener_caller_abort_releases_port() {
    let _ports = PORTS.lock().await;
    for outcome in [None, Some(Ok(()))] {
        let account = account();
        let (login, mut opened) = start(&account, outcome);
        let url = opened.recv().await.unwrap();
        login.abort();
        assert!(login.await.unwrap_err().is_cancelled());
        account.logout().await.unwrap();
        let _finished = account.operation.lock().await;
        assert_port_free(&url);
        let status = account.status().await;
        assert!(!status.pending);
        assert!(status.identity.is_none());
    }
}

#[tokio::test]
async fn callback_listener_caller_abort_can_be_superseded() {
    let _ports = PORTS.lock().await;
    let account = account();
    let (old, mut opened) = start(&account, None);
    let old_url = opened.recv().await.unwrap();
    old.abort();
    assert!(old.await.unwrap_err().is_cancelled());
    let (replacement, mut opened) = start(&account, Some(Ok(())));
    let new_url = opened.recv().await.unwrap();
    assert_port_free(&old_url);
    assert_eq!(
        account
            .complete(Callback {
                code: "c".repeat(43),
                state: parameter(&old_url, "state")
            })
            .await,
        Err(Error::WrongState)
    );
    complete_http(&new_url).await;
    assert!(replacement.await.unwrap().unwrap().identity.is_some());
    assert_port_free(&new_url);
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
    let (login, mut opened) = start(&account, Some(Ok(())));
    let url = opened.recv().await.unwrap();
    complete_http(&url).await;
    assert!(login.await.unwrap().unwrap().identity.is_some());
    assert_port_free(&url);
}
