use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct Context {
    platform: &'static str,
    os_name: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    os_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    app_version: Option<crate::Version>,
    language: String,
}

impl Context {
    pub fn new(version: &str, os_version: Option<String>, locale: Option<&str>) -> Self {
        Self {
            platform: "Desktop",
            os_name: match std::env::consts::OS {
                "windows" => "Windows",
                "macos" => "macOS",
                "linux" => "Linux",
                _ => "Other",
            },
            os_version: os_version.filter(|value| !value.is_empty()),
            app_version: crate::Version::from_release(version),
            language: locale
                .and_then(|value| value.split(['_', '-', '.']).next())
                .filter(|value| {
                    (2..=3).contains(&value.len()) && value.bytes().all(|c| c.is_ascii_alphabetic())
                })
                .map(|value| value.to_ascii_lowercase())
                .unwrap_or_else(|| "und".into()),
        }
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new("unknown", None, None)
    }
}
