use crate::{Config, Event, Feature};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    io::Write,
    path::Path,
};
use uuid::Uuid;

pub const QUEUE_CAP: usize = 1000;
pub const BATCH_CAP: usize = 50;
pub const RETRY_DELAYS_MS: [u64; 5] = [30_000, 60_000, 120_000, 240_000, 480_000];

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Consent {
    #[default]
    Off,
    On,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counters {
    pub dropped_oldest: u64,
    pub dropped_payload: u64,
    pub retry_exhausted: u64,
    pub dropped_config: u64,
    pub delivered: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Queued {
    pub insert_id: Uuid,
    pub time: u64,
    pub user_id: Option<String>,
    pub event: Event,
    pub failures: usize,
    pub retry_at: u64,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct State {
    pub consent: Consent,
    pub device_id: Option<Uuid>,
    pub user_id: Option<String>,
    pub last_active_day: Option<chrono::NaiveDate>,
    pub feature_days: BTreeMap<Feature, chrono::NaiveDate>,
    pub config: Config,
    pub queue: VecDeque<Queued>,
    pub counters: Counters,
}

impl State {
    pub fn clear(&mut self) {
        self.consent = Consent::Off;
        self.device_id = None;
        self.user_id = None;
        self.queue.clear();
        self.last_active_day = None;
        self.feature_days.clear();
    }

    pub fn read(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let meta = std::fs::metadata(path).map_err(|_| "analytics settings unavailable")?;
        if meta.len() > 2_000_000 {
            return Err("analytics settings exceed size bound".into());
        }
        let bytes = std::fs::read(path).map_err(|_| "analytics settings unavailable")?;
        let state: Self =
            serde_json::from_slice(&bytes).map_err(|_| "invalid analytics settings")?;
        if state.consent == Consent::On
            && state.device_id.is_none_or(|id| id.get_version_num() != 4)
        {
            return Err("invalid analytics device identity".into());
        }
        if state.user_id.as_deref().is_some_and(|uid| !valid_uid(uid))
            || state.queue.iter().any(|event| {
                event.failures > RETRY_DELAYS_MS.len()
                    || event.user_id.as_deref().is_some_and(|uid| !valid_uid(uid))
            })
        {
            return Err("invalid analytics queue".into());
        }
        Ok(state)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let parent = path
            .parent()
            .ok_or("analytics settings need a parent directory")?;
        std::fs::create_dir_all(parent).map_err(|_| "analytics settings directory unavailable")?;
        let mut file = tempfile::NamedTempFile::new_in(parent)
            .map_err(|_| "analytics settings write failed")?;
        serde_json::to_writer(&mut file, self).map_err(|_| "analytics settings encoding failed")?;
        file.flush()
            .map_err(|_| "analytics settings flush failed")?;
        file.as_file()
            .sync_all()
            .map_err(|_| "analytics settings sync failed")?;
        file.persist(path)
            .map_err(|_| "analytics settings replacement failed")?;
        Ok(())
    }
}

pub(crate) fn valid_uid(uid: &str) -> bool {
    (5..=128).contains(&uid.len())
        && uid
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}
