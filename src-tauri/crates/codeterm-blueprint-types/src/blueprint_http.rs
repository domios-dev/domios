use crate::blueprint_source::SourceReading;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpProbe {
    pub url: String,
    #[serde(default = "default_expected")]
    pub expected_status: String,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}
fn default_expected() -> String {
    "2xx".into()
}
fn default_timeout() -> u64 {
    3000
}

impl HttpProbe {
    pub fn validate(&self) -> Result<(), String> {
        let url = url::Url::parse(&self.url)
            .map_err(|_| "HTTP requires an absolute http or https URL")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || self.url.len() > 4096
            || self.url.chars().any(char::is_control)
        {
            return Err("HTTP requires a bounded http or https URL without credentials".into());
        }
        if self.expected_status != "2xx"
            && !self
                .expected_status
                .parse::<u16>()
                .is_ok_and(|code| (100..=599).contains(&code))
        {
            return Err("HTTP expected_status must be 2xx or a code from 100 to 599".into());
        }
        if !(1..=10000).contains(&self.timeout_ms) {
            return Err("HTTP timeout_ms must be 1..10000".into());
        }
        Ok(())
    }
    pub fn display_url(&self) -> String {
        let Ok(mut url) = url::Url::parse(&self.url) else {
            return "HTTP endpoint".into();
        };
        url.set_query(None);
        url.set_fragment(None);
        url.to_string()
    }
    pub fn reading(&self, code: Option<u16>) -> SourceReading {
        let up = code.is_some_and(|code| {
            if self.expected_status == "2xx" {
                (200..300).contains(&code)
            } else {
                self.expected_status.parse::<u16>() == Ok(code)
            }
        });
        let state = if up { "up" } else { "down" };
        let detail = code.map_or_else(|| "no response".into(), |code| code.to_string());
        SourceReading {
            state: state.into(),
            summary: format!("{state} · {detail}"),
            output: json!({"state":state,"code":code}),
        }
    }
}

#[cfg(test)]
mod tests;
