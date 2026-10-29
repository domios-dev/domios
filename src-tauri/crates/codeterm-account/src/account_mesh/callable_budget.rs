use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::{Duration, Instant, SystemTime},
};

#[derive(Default)]
pub(super) struct Budget {
    failures: HashMap<String, u32>,
    retry_at: Option<Instant>,
}

impl Budget {
    pub fn remaining(&self, now: Instant) -> Duration {
        self.retry_at
            .map(|at| at.saturating_duration_since(now))
            .unwrap_or_default()
    }

    pub fn limited(&mut self, route: &str, now: Instant, retry_after_seconds: u64) {
        let failures = self.failures.entry(route.to_owned()).or_default();
        let backoff = (1u64 << (*failures).min(9)).min(300);
        *failures = failures.saturating_add(1);
        let delay = Duration::from_secs(retry_after_seconds.max(backoff));
        let at = now
            .checked_add(delay)
            .unwrap_or_else(|| now + Duration::from_secs(u32::MAX as u64));
        self.retry_at = Some(self.retry_at.map_or(at, |previous| previous.max(at)));
    }

    pub fn success(&mut self, route: &str) {
        self.failures.remove(route);
    }
}

struct AccountBudget {
    state: Arc<tokio::sync::Mutex<Budget>>,
    probe: Arc<tokio::sync::Semaphore>,
}

fn account_budget(uid: &str) -> Arc<AccountBudget> {
    static ACCOUNTS: OnceLock<parking_lot::Mutex<HashMap<String, Arc<AccountBudget>>>> =
        OnceLock::new();
    ACCOUNTS
        .get_or_init(Default::default)
        .lock()
        .entry(uid.to_owned())
        .or_insert_with(|| {
            Arc::new(AccountBudget {
                state: Default::default(),
                probe: Arc::new(tokio::sync::Semaphore::new(1)),
            })
        })
        .clone()
}

#[cfg(test)]
pub(super) fn account(uid: &str) -> Arc<tokio::sync::Mutex<Budget>> {
    account_budget(uid).state.clone()
}

#[async_trait::async_trait]
pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
    async fn wait(&self, delay: Duration);
}

pub(super) struct RuntimeClock;
#[async_trait::async_trait]
impl Clock for RuntimeClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
    async fn wait(&self, delay: Duration) {
        tokio::time::sleep(delay).await;
    }
}

pub(super) async fn run<T: super::callable::CallableTransport + ?Sized>(
    transport: &T,
    uid: &str,
    route: &str,
    id_token: &str,
    data: serde_json::Value,
    guard: &dyn super::callable::CallableGuard,
    clock: &dyn Clock,
) -> Result<serde_json::Value, super::callable::CallableFailure> {
    use super::callable::CallableFailure;
    let admission = account_budget(uid);
    let budget = &admission.state;
    let _probe = if guard.closing_session() {
        None
    } else {
        let permit = tokio::select! {
            biased;
            _ = guard.retired() => return Err(CallableFailure::Retired),
            permit = admission.probe.acquire() => permit.map_err(|_| CallableFailure::Retired)?,
        };
        tokio::select! {
            biased;
            _ = guard.retired() => return Err(CallableFailure::Retired),
            _ = async {
                loop {
                    let delay = budget.lock().await.remaining(clock.now());
                    if delay.is_zero() { break; }
                    clock.wait(delay).await;
                }
            } => {},
        }
        Some(permit)
    };
    if !guard.current().await {
        return Err(CallableFailure::Retired);
    }
    let response = tokio::select! {
        biased;
        _ = guard.retired() => return Err(CallableFailure::Retired),
        response = tokio::time::timeout(Duration::from_secs(15), transport.call(route, id_token, data)) => {
            response.unwrap_or(Err(CallableFailure::Offline))
        },
    };
    if let Err(failure @ CallableFailure::Status { status: 429, .. }) = &response {
        budget.lock().await.limited(
            route,
            clock.now(),
            failure.retry_after_seconds().unwrap_or(0),
        );
    }
    if response.is_ok() {
        budget.lock().await.success(route);
    }
    response
}

pub(super) fn retry_after(value: &str, now: SystemTime) -> Option<u64> {
    if let Ok(seconds) = value.trim().parse() {
        return Some(seconds);
    }
    let parts: Vec<_> = value.split_ascii_whitespace().collect();
    let [_, day, month, year, time, "GMT"] = parts.as_slice() else {
        return None;
    };
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|candidate| candidate == month)? as i64
        + 1;
    let day: i64 = day.parse().ok()?;
    let year: i64 = year.parse().ok()?;
    let time: Vec<_> = time
        .split(':')
        .map(str::parse::<i64>)
        .collect::<Result<_, _>>()
        .ok()?;
    let [hour, minute, second] = time.as_slice() else {
        return None;
    };
    if !(1970..=9999).contains(&year)
        || !(1..=31).contains(&day)
        || !(0..=23).contains(hour)
        || !(0..=59).contains(minute)
        || !(0..=60).contains(second)
    {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day > days_in_month {
        return None;
    }
    let year = year - i64::from(month <= 2);
    let era = year / 400;
    let yoe = year - era * 400;
    let shifted_month = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * shifted_month + 2) / 5 + day - 1;
    let days = era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468;
    let epoch = u64::try_from(days * 86400 + hour * 3600 + minute * 60 + second).ok()?;
    let target = SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(epoch))?;
    Some(
        target
            .duration_since(now)
            .map(|delay| delay.as_secs() + u64::from(delay.subsec_nanos() > 0))
            .unwrap_or(0),
    )
}

#[cfg(test)]
mod tests;
