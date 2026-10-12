use crate::{engine::Engine, Config, Consent, Event, Status};
use std::{
    path::PathBuf,
    sync::{mpsc, Arc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::oneshot;

pub type SecretReader = Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;

enum Command {
    Status(oneshot::Sender<Status>),
    Consent(Consent, oneshot::Sender<Result<Status, String>>),
    Event(Event, oneshot::Sender<Result<bool, String>>),
    Identity(Option<String>, oneshot::Sender<Result<(), String>>),
}

#[derive(Clone)]
pub struct Worker {
    sender: mpsc::SyncSender<Command>,
}

impl Worker {
    pub fn start(path: PathBuf, config: Config, secrets: SecretReader) -> Result<Self, String> {
        let mut engine = Engine::open(path, config)?;
        engine.sink = crate::sinks::from_config(config, secrets)?;
        let (sender, receiver) = mpsc::sync_channel(64);
        std::thread::Builder::new()
            .name("analytics".into())
            .spawn(move || run(engine, receiver))
            .map_err(|_| "analytics worker unavailable")?;
        Ok(Self { sender })
    }

    fn send(&self, command: Command) -> Result<(), String> {
        self.sender
            .try_send(command)
            .map_err(|_| "analytics worker busy or stopped".into())
    }

    pub async fn status(&self) -> Result<Status, String> {
        let (sender, receiver) = oneshot::channel();
        self.send(Command::Status(sender))?;
        receiver
            .await
            .map_err(|_| "analytics worker stopped".into())
    }

    pub async fn set_consent(&self, consent: Consent) -> Result<Status, String> {
        let (sender, receiver) = oneshot::channel();
        self.send(Command::Consent(consent, sender))?;
        receiver.await.map_err(|_| "analytics worker stopped")?
    }

    pub async fn record(&self, event: Event) -> Result<bool, String> {
        let (sender, receiver) = oneshot::channel();
        self.send(Command::Event(event, sender))?;
        receiver.await.map_err(|_| "analytics worker stopped")?
    }

    pub async fn set_user_id(&self, account_uid: Option<String>) -> Result<(), String> {
        let (sender, receiver) = oneshot::channel();
        self.send(Command::Identity(account_uid, sender))?;
        receiver.await.map_err(|_| "analytics worker stopped")?
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn run(mut engine: Engine, receiver: mpsc::Receiver<Command>) {
    let mut suspended = false;
    loop {
        let deadline = if suspended { None } else { engine.deadline() };
        let command = match receiver.try_recv() {
            Ok(command) => Some(command),
            Err(mpsc::TryRecvError::Disconnected) => break,
            Err(mpsc::TryRecvError::Empty) => match deadline {
                None => match receiver.recv() {
                    Ok(command) => Some(command),
                    Err(_) => break,
                },
                Some(at) => match receiver
                    .recv_timeout(Duration::from_millis(at.saturating_sub(now_ms())))
                {
                    Ok(command) => Some(command),
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                },
            },
        };
        match command {
            Some(Command::Status(reply)) => {
                let _ = reply.send(engine.status());
            }
            Some(Command::Consent(consent, reply)) => {
                suspended = false;
                let _ = reply.send(engine.consent(consent));
            }
            Some(Command::Event(event, reply)) => {
                suspended = false;
                let _ = reply.send(engine.enqueue(event, now_ms()));
            }
            Some(Command::Identity(uid, reply)) => {
                suspended = false;
                let _ = reply.send(engine.identity(uid));
            }
            None => {
                suspended = engine.deliver(now_ms()).is_err();
            }
        }
    }
}
