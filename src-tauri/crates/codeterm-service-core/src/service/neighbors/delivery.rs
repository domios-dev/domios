use codeterm_neighbors::Error;
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Rejection {
    pub reason: &'static str,
    pub limit: Option<u64>,
    pub resets_at: Option<u64>,
    pub retry_after_seconds: Option<u64>,
}

impl Rejection {
    pub(super) fn from_error(error: Error) -> Option<Self> {
        let mut rejection = Self {
            reason: "",
            limit: None,
            resets_at: None,
            retry_after_seconds: None,
        };
        rejection.reason = match error {
            Error::DailyLimit { limit, resets_at } => {
                rejection.limit = Some(limit);
                rejection.resets_at = Some(resets_at);
                "neighbor_daily_limit"
            }
            Error::RateLimited {
                retry_after_seconds,
            } => {
                rejection.retry_after_seconds = Some(retry_after_seconds);
                "rate_limited"
            }
            Error::AggregateLimited {
                retry_after_seconds,
            } => {
                rejection.retry_after_seconds = Some(retry_after_seconds);
                "aggregate_limited"
            }
            Error::Locked => "friends_locked",
            Error::RecipientLocked => "recipient_locked",
            Error::RecipientNotReceiving => "recipient_not_receiving",
            _ => return None,
        };
        Some(rejection)
    }

    pub(super) fn receipt(self, message: &str) -> Value {
        json!({"message_id":message,"status":"not_delivered","reason":self.reason,"limit":self.limit,"resets_at":self.resets_at,"retry_after_seconds":self.retry_after_seconds})
    }

    pub(super) fn description(self) -> String {
        match self.reason {
            "neighbor_daily_limit" => {
                let time = self
                    .resets_at
                    .and_then(|time| i64::try_from(time).ok())
                    .and_then(|time| time.checked_mul(1000))
                    .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
                    .map(|time| time.format("%Y-%m-%d %H:%M UTC").to_string());
                time.map(|time| format!("daily limit reached, resets {time}"))
                    .unwrap_or_else(|| "daily limit reached".into())
            }
            "rate_limited" => format!(
                "hourly limit reached, retry in {} seconds",
                self.retry_after_seconds.unwrap_or_default()
            ),
            "aggregate_limited" => format!(
                "plan send limit reached, retry in {} seconds",
                self.retry_after_seconds.unwrap_or_default()
            ),
            "friends_locked" => "your plan does not allow this channel".into(),
            "recipient_locked" => "their plan does not allow this channel".into(),
            "recipient_not_receiving" => "recipient is not receiving".into(),
            _ => "send was refused".into(),
        }
    }
}

pub(super) struct SendResult {
    pub receipt: Value,
    pub rejection: Option<Rejection>,
}

#[cfg(test)]
mod tests;
