use crate::{engine::Engine, AnalyticsSink, Config, Consent, Event, Status};
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
    Activity(Event, chrono::NaiveDate),
    Identity(Option<String>, oneshot::Sender<Result<(), String>>),
    Flush(oneshot::Sender<Result<(), String>>),
}

#[derive(Clone)]
pub struct Worker {
    sender: mpsc::SyncSender<Command>,
    activity: Arc<crate::activity::Activity>,
}

impl Worker {
    pub fn start(path: PathBuf, config: Config, secrets: SecretReader) -> Result<Self, String> {
        let mut engine = Engine::open(path, config)?;
        engine.sink = crate::sinks::from_config(config, secrets)?;
        Self::spawn(engine)
    }

    pub fn start_with_sink(
        path: PathBuf,
        config: Config,
        sink: Box<dyn AnalyticsSink>,
    ) -> Result<Self, String> {
        let mut engine = Engine::open(path, config)?;
        engine.sink = sink;
        Self::spawn(engine)
    }

    fn spawn(engine: Engine) -> Result<Self, String> {
        let activity = Arc::new(crate::activity::Activity::new(
            engine.state.consent,
            engine.state.last_active_day,
        ));
        let shared_activity = activity.clone();
        let (sender, receiver) = mpsc::sync_channel(64);
        std::thread::Builder::new()
            .name("analytics".into())
            .spawn(move || run(engine, receiver, shared_activity))
            .map_err(|_| "analytics worker unavailable")?;
        Ok(Self { sender, activity })
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

    pub fn record_nowait(&self, event: Event) -> Result<(), String> {
        let (sender, _receiver) = oneshot::channel();
        self.send(Command::Event(event, sender))
    }

    pub fn set_mesh_machines(&self, machines: usize) {
        self.activity.set_machines(machines);
    }

    pub fn record_activity_nowait(
        &self,
        workspaces: impl FnOnce() -> usize,
    ) -> Result<bool, String> {
        self.record_activity_on_day(chrono::Local::now().date_naive(), workspaces)
    }

    pub(crate) fn record_activity_on_day(
        &self,
        day: chrono::NaiveDate,
        workspaces: impl FnOnce() -> usize,
    ) -> Result<bool, String> {
        if !self.activity.claim(day) {
            return Ok(false);
        }
        let event = Event::ActiveDay {
            mesh_machines: u32::try_from(self.activity.machines()).unwrap_or(u32::MAX),
            workspaces: u32::try_from(workspaces()).unwrap_or(u32::MAX),
        };
        if let Err(error) = self.send(Command::Activity(event, day)) {
            self.activity.release();
            return Err(error);
        }
        Ok(true)
    }

    pub async fn record(&self, event: Event) -> Result<bool, String> {
        let (sender, receiver) = oneshot::channel();
        self.send(Command::Event(event, sender))?;
        receiver.await.map_err(|_| "analytics worker stopped")?
    }

    pub async fn flush(&self) -> Result<(), String> {
        let (sender, receiver) = oneshot::channel();
        self.send(Command::Flush(sender))?;
        receiver.await.map_err(|_| "analytics worker stopped")?
    }

    pub async fn set_user_id(&self, account_uid: Option<String>) -> Result<(), String> {
        let (sender, receiver) = oneshot::channel();
        if account_uid
            .as_deref()
            .is_some_and(|value| !crate::state::valid_uid(value))
        {
            return Err("invalid account uid".into());
        }
        let commands = self.sender.clone();
        tokio::task::spawn_blocking(move || commands.send(Command::Identity(account_uid, sender)))
            .await
            .map_err(|_| "analytics identity unavailable")?
            .map_err(|_| "analytics worker stopped")?;
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

fn run(
    mut engine: Engine,
    receiver: mpsc::Receiver<Command>,
    activity: Arc<crate::activity::Activity>,
) {
    let mut suspended = false;
    let mut account_uid = None;
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
                let previous = engine.state.consent;
                let result = engine.consent(consent).and_then(|_| {
                    if consent == Consent::On {
                        engine.identity(account_uid.clone())?;
                        if previous == Consent::Off {
                            engine.enqueue(Event::AnalyticsOptedIn {}, now_ms())?;
                        }
                    }
                    Ok(engine.status())
                });
                activity.publish(engine.state.consent, engine.state.last_active_day);
                let _ = reply.send(result);
            }
            Some(Command::Event(event, reply)) => {
                suspended = false;
                let result = engine.enqueue(event, now_ms());
                activity.publish(engine.state.consent, engine.state.last_active_day);
                let _ = reply.send(result);
            }
            Some(Command::Activity(event, day)) => {
                suspended = false;
                let _ = engine.enqueue_on_day(event, now_ms(), day);
                activity.publish(engine.state.consent, engine.state.last_active_day);
                activity.release();
            }
            Some(Command::Identity(uid, reply)) => {
                suspended = false;
                account_uid = uid.clone();
                let result = engine.identity(uid);
                activity.publish(engine.state.consent, engine.state.last_active_day);
                let _ = reply.send(result);
            }
            Some(Command::Flush(reply)) => {
                let result = engine.deliver(now_ms());
                activity.publish(engine.state.consent, engine.state.last_active_day);
                let _ = reply.send(result);
            }
            None => {
                suspended = engine.deliver(now_ms()).is_err();
                activity.publish(engine.state.consent, engine.state.last_active_day);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_full_queue_releases_claim_for_next_activity() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        let worker = Worker {
            sender,
            activity: Arc::new(crate::activity::Activity::new(Consent::On, None)),
        };
        worker.record_nowait(Event::AnalyticsOptedIn {}).unwrap();
        let day = chrono::NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        assert!(worker.record_activity_on_day(day, || 1).is_err());
        assert!(worker.activity.claim(day));
    }
}
