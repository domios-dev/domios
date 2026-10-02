use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use serde::{Deserialize, Serialize};

use super::machine::{self, Machine, Route};
use crate::cron::clock::SharedClock;
use crate::persisted_state::{Loaded, StateFile};

const MAX_SENTINELS: usize = 256;
const MAX_ACTIVE: usize = 64;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Create {
    pub probe: String,
    pub question: String,
    pub buckets: Vec<String>,
    #[serde(default = "default_every")]
    pub every: String,
    #[serde(default)]
    pub routes: BTreeMap<String, String>,
    #[serde(default)]
    pub matches: BTreeMap<String, String>,
    pub until: Option<String>,
    pub timeout: Option<String>,
}

fn default_every() -> String {
    "20s".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sentinel {
    pub id: String,
    pub team_id: Option<String>,
    pub creator_tab_id: String,
    pub workspace_id: String,
    pub cwd: String,
    pub probe: String,
    pub question: String,
    pub buckets: Vec<String>,
    pub interval_ms: u64,
    pub routes: BTreeMap<String, Route>,
    pub matches: BTreeMap<String, String>,
    pub until: Option<String>,
    pub expires_at: Option<u64>,
    pub next_run: u64,
    pub machine: Machine,
    pub last_reason: Option<String>,
    #[serde(default)]
    pub revision: u64,
}

#[derive(Clone)]
pub struct Owner {
    pub creator_tab_id: String,
    pub team_id: Option<String>,
    pub workspace_id: String,
    pub cwd: String,
}

pub struct Lease {
    pub job: Sentinel,
    pub cancelled: Arc<AtomicBool>,
}

pub struct Store {
    jobs: Vec<Sentinel>,
    active: HashMap<String, Arc<AtomicBool>>,
    file: StateFile,
    pub clock: SharedClock,
}

pub fn duration_ms(value: &str) -> Result<u64, String> {
    if value.len() > 32
        || !value
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, 's' | 'm' | 'h' | 'd'))
    {
        return Err("duration: use seconds, minutes, hours or days, e.g. 20s or 1h30m".into());
    }
    if value.chars().any(|c| c.is_ascii_alphabetic())
        && value.ends_with(|c: char| c.is_ascii_digit())
    {
        return Err("duration: every component needs a unit".into());
    }
    let mut number = String::new();
    let mut bound = 0u64;
    for c in value.chars() {
        if c.is_ascii_digit() {
            number.push(c);
            continue;
        }
        let n: u64 = number.parse().map_err(|_| "duration: invalid number")?;
        number.clear();
        let factor = match c {
            's' => 1000,
            'm' => 60_000,
            'h' => 3_600_000,
            'd' => 86_400_000,
            _ => unreachable!(),
        };
        bound = bound
            .checked_add(n.checked_mul(factor).ok_or("duration: overflow")?)
            .ok_or("duration: overflow")?;
    }
    if !number.is_empty() {
        bound = bound
            .checked_add(
                number
                    .parse::<u64>()
                    .map_err(|_| "duration: invalid number")?
                    .checked_mul(60_000)
                    .ok_or("duration: overflow")?,
            )
            .ok_or("duration: overflow")?;
    }
    if bound == 0 || bound > 366 * 86_400_000 {
        return Err("duration: must be positive and at most 366 days".into());
    }
    crate::cron::store::parse_oneshot_schedule_at(&format!("in {value}"), 0)
}

fn decode(raw: &str) -> Result<Vec<Sentinel>, String> {
    let jobs: Vec<Sentinel> = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if jobs.len() > MAX_SENTINELS {
        return Err("sentinel store exceeds capacity".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for job in &jobs {
        machine::validate(&job.buckets, &job.matches, true, job.interval_ms)?;
        if !ids.insert(&job.id)
            || job.creator_tab_id.is_empty()
            || job.workspace_id.is_empty()
            || job.cwd.is_empty()
        {
            return Err("invalid sentinel identity or scope".into());
        }
        if job.probe.trim().is_empty()
            || job.probe.len() > 8192
            || job.question.trim().is_empty()
            || job.question.len() > 4096
        {
            return Err("invalid sentinel probe or question".into());
        }
        if job
            .routes
            .keys()
            .any(|bucket| !job.buckets.contains(bucket))
            || job
                .until
                .as_ref()
                .is_some_and(|bucket| !job.buckets.contains(bucket))
        {
            return Err("invalid sentinel route bucket".into());
        }
    }
    Ok(jobs)
}

impl Store {
    pub fn new(path: PathBuf, clock: SharedClock) -> Self {
        let file = StateFile::new(path).with_validator(|raw| decode(raw).map(|_| ()));
        let jobs = match file.load(decode) {
            Loaded::Parsed(jobs) | Loaded::Recovered { value: jobs, .. } => jobs,
            Loaded::Missing | Loaded::Unrecoverable { .. } => Vec::new(),
        };
        Self {
            jobs,
            active: HashMap::new(),
            file,
            clock,
        }
    }

    fn save(&self) -> Result<(), String> {
        let raw = serde_json::to_string_pretty(&self.jobs).map_err(|e| e.to_string())?;
        self.file.save(&raw)
    }

    pub fn list(&self) -> &[Sentinel] {
        &self.jobs
    }
    pub fn get(&self, id: &str) -> Option<&Sentinel> {
        self.jobs.iter().find(|job| job.id == id)
    }

    pub fn prepare(
        &self,
        params: Create,
        owner: Owner,
        connected: bool,
    ) -> Result<Sentinel, String> {
        let Owner {
            creator_tab_id: creator,
            team_id: team,
            workspace_id: workspace,
            cwd,
        } = owner;
        let interval_ms = duration_ms(&params.every)?;
        machine::validate(&params.buckets, &params.matches, connected, interval_ms)?;
        if params.probe.trim().is_empty()
            || params.probe.len() > 8192
            || params.question.trim().is_empty()
            || params.question.len() > 4096
        {
            return Err(
                "--probe and --question must be nonempty and bounded to 8192/4096 bytes".into(),
            );
        }
        if self.jobs.len() >= MAX_SENTINELS
            || self.jobs.iter().filter(|job| !job.machine.stopped).count() >= MAX_ACTIVE
        {
            return Err("sentinel capacity reached (64 active, 256 total); stop active sentinels or close their Team".into());
        }
        if params
            .until
            .as_ref()
            .is_some_and(|bucket| !params.buckets.contains(bucket))
        {
            return Err("--until: name a declared bucket".into());
        }
        let mut routes = BTreeMap::new();
        for (bucket, action) in params.routes {
            if !params.buckets.contains(&bucket) {
                return Err("--route: name a declared bucket".into());
            }
            routes.insert(bucket, Route::parse(&action)?);
        }
        let now = self.clock.now_ms();
        let expires_at = params
            .timeout
            .as_deref()
            .map(duration_ms)
            .transpose()?
            .map(|duration| now.saturating_add(duration));
        let job = Sentinel {
            id: format!(
                "sentinel_{}",
                &uuid::Uuid::new_v4().simple().to_string()[..12]
            ),
            team_id: team,
            creator_tab_id: creator,
            workspace_id: workspace,
            cwd,
            probe: params.probe,
            question: params.question,
            buckets: params.buckets,
            interval_ms,
            routes,
            matches: params.matches,
            until: params.until,
            expires_at,
            next_run: now,
            machine: Machine::default(),
            last_reason: None,
            revision: 0,
        };
        Ok(job)
    }

    pub fn insert(&mut self, job: Sentinel) -> Result<Sentinel, String> {
        if self.jobs.len() >= MAX_SENTINELS
            || self.jobs.iter().filter(|job| !job.machine.stopped).count() >= MAX_ACTIVE
        {
            return Err("sentinel capacity reached".into());
        }
        self.jobs.push(job.clone());
        if let Err(error) = self.save() {
            self.jobs.pop();
            return Err(error);
        }
        Ok(job)
    }

    #[cfg(test)]
    pub fn create(
        &mut self,
        params: Create,
        owner: Owner,
        connected: bool,
    ) -> Result<Sentinel, String> {
        let job = self.prepare(params, owner, connected)?;
        self.insert(job)
    }

    pub fn stop(&mut self, id: &str, reason: &str) -> Result<Sentinel, String> {
        let job = self
            .jobs
            .iter_mut()
            .find(|job| job.id == id)
            .ok_or_else(|| format!("sentinel not found: {id}"))?;
        job.machine.stopped = true;
        job.revision += 1;
        job.last_reason = Some(reason.into());
        if let Some(cancelled) = self.active.remove(id) {
            cancelled.store(true, Ordering::SeqCst);
        }
        let result = job.clone();
        self.save()?;
        Ok(result)
    }

    pub fn remove_team(&mut self, team: &str) -> Result<(), String> {
        for job in self
            .jobs
            .iter()
            .filter(|job| job.team_id.as_deref() == Some(team))
        {
            if let Some(cancelled) = self.active.remove(&job.id) {
                cancelled.store(true, Ordering::SeqCst);
            }
        }
        self.jobs.retain(|job| job.team_id.as_deref() != Some(team));
        self.save()
    }

    pub fn due(&mut self) -> Result<Vec<Lease>, String> {
        let now = self.clock.now_ms();
        let mut due = Vec::new();
        let mut changed = false;
        for job in &mut self.jobs {
            if job.machine.stopped {
                continue;
            }
            if job.expires_at.is_some_and(|at| now >= at) {
                job.machine.stopped = true;
                job.revision += 1;
                job.last_reason = Some("timeout".into());
                if let Some(cancelled) = self.active.remove(&job.id) {
                    cancelled.store(true, Ordering::SeqCst);
                }
                changed = true;
                continue;
            }
            if job.next_run <= now && !self.active.contains_key(&job.id) {
                let missed = (now - job.next_run) / job.interval_ms;
                job.next_run = job
                    .next_run
                    .saturating_add((missed + 1).saturating_mul(job.interval_ms));
                let cancelled = Arc::new(AtomicBool::new(false));
                self.active.insert(job.id.clone(), cancelled.clone());
                due.push(Lease {
                    job: job.clone(),
                    cancelled,
                });
                changed = true;
            }
        }
        if changed {
            if let Err(error) = self.save() {
                for lease in &due {
                    lease.cancelled.store(true, Ordering::SeqCst);
                    self.active.remove(&lease.job.id);
                }
                return Err(error);
            }
        }
        Ok(due)
    }

    pub fn finish(&mut self, job: &Sentinel) -> Result<bool, String> {
        let Some(current) = self.jobs.iter_mut().find(|current| current.id == job.id) else {
            return Ok(false);
        };
        if current.revision != job.revision
            || current.machine.stopped
            || current
                .expires_at
                .is_some_and(|at| self.clock.now_ms() >= at)
        {
            return Ok(false);
        }
        *current = job.clone();
        if let Err(error) = self.save() {
            if let Some(current) = self.jobs.iter_mut().find(|current| current.id == job.id) {
                current.machine.stopped = true;
                current.last_reason = Some("persistence_failed".into());
            }
            return Err(error);
        }
        Ok(true)
    }

    pub fn release(&mut self, id: &str) {
        self.active.remove(id);
    }

    pub fn next_deadline(&self) -> u64 {
        let now = self.clock.now_ms();
        self.jobs
            .iter()
            .filter(|job| !job.machine.stopped)
            .flat_map(|job| {
                let next = if self.active.contains_key(&job.id) {
                    None
                } else {
                    Some(job.next_run)
                };
                next.into_iter().chain(job.expires_at)
            })
            .min()
            .map(|at| at.saturating_sub(now))
            .unwrap_or(60_000)
            .min(60_000)
    }
}
