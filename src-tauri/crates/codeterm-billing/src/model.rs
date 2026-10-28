use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    Free,
    Pro,
    Max,
    Sponsor,
    Admin,
    Paid(String),
}
impl Plan {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Free => "free",
            Self::Pro => "pro",
            Self::Max => "max",
            Self::Sponsor => "sponsor",
            Self::Admin => "admin",
            Self::Paid(name) => name,
        }
    }
}
impl From<&codeterm_entitlements::Label> for Plan {
    fn from(label: &codeterm_entitlements::Label) -> Self {
        Self::from_name(label.as_str().to_owned())
    }
}
impl Plan {
    fn from_name(name: String) -> Self {
        match name.as_str() {
            "free" => Self::Free,
            "pro" => Self::Pro,
            "max" => Self::Max,
            "sponsor" => Self::Sponsor,
            "admin" => Self::Admin,
            _ => Self::Paid(name),
        }
    }
}
impl Serialize for Plan {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}
impl<'de> Deserialize<'de> for Plan {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        if name.is_empty() || name.chars().count() > 128 || name.chars().any(char::is_control) {
            return Err(serde::de::Error::custom("Invalid tier label"));
        }
        Ok(Self::from_name(name))
    }
}

/// A commercial price key sent to checkout unchanged; it maps to an entitlement plan only for analytics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckoutKey {
    ProMonthly,
    ProYearly,
    MaxMonthly,
    MaxYearly,
    SponsorMonthly,
    SponsorYearly,
}
impl CheckoutKey {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "pro_monthly" => Ok(Self::ProMonthly),
            "pro_yearly" => Ok(Self::ProYearly),
            "max_monthly" => Ok(Self::MaxMonthly),
            "max_yearly" => Ok(Self::MaxYearly),
            "sponsor_monthly" => Ok(Self::SponsorMonthly),
            "sponsor_yearly" => Ok(Self::SponsorYearly),
            _ => Err("Unknown billing plan".into()),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProMonthly => "pro_monthly",
            Self::ProYearly => "pro_yearly",
            Self::MaxMonthly => "max_monthly",
            Self::MaxYearly => "max_yearly",
            Self::SponsorMonthly => "sponsor_monthly",
            Self::SponsorYearly => "sponsor_yearly",
        }
    }
    pub fn plan(self) -> Plan {
        match self {
            Self::ProMonthly | Self::ProYearly => Plan::Pro,
            Self::MaxMonthly | Self::MaxYearly => Plan::Max,
            Self::SponsorMonthly | Self::SponsorYearly => Plan::Sponsor,
        }
    }
}

/// The app's resolved appearance, so the website checkout opens in the same theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckoutTheme {
    Light,
    Dark,
}
impl CheckoutTheme {
    /// Unknown values (`system`, other casings, future themes) open the plain checkout rather than failing.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

/// Sets `theme` on a checkout URL, keeping every other parameter (`_ptxn` included) and the fragment.
pub fn checkout_url_with_theme(mut url: url::Url, theme: CheckoutTheme) -> url::Url {
    // Works on the raw segments so every other parameter keeps its exact bytes.
    let mut segments: Vec<String> = url
        .query()
        .unwrap_or_default()
        .split('&')
        .filter(|segment| !segment.is_empty() && segment.split('=').next() != Some("theme"))
        .map(str::to_owned)
        .collect();
    segments.push(format!("theme={}", theme.as_str()));
    url.set_query(Some(&segments.join("&")));
    url
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Active,
    Trialing,
    PastDue,
    Canceled,
    Revoked,
    Incomplete,
    Unpaid,
    Paused,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entitlements {
    pub plan: Plan,
    pub status: Status,
    pub current_period_end: u64,
    pub cancel_at_period_end: bool,
}
impl Default for Entitlements {
    fn default() -> Self {
        Self {
            plan: Plan::Free,
            status: Status::Active,
            current_period_end: 0,
            cancel_at_period_end: false,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Terminal,
    PaidFeatures,
}
