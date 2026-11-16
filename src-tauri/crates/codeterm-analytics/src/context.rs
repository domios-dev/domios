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
    #[serde(skip_serializing_if = "Option::is_none")]
    device_manufacturer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_model: Option<String>,
    carrier: &'static str,
}

impl Context {
    pub fn new(version: &str, os_version: Option<String>, locale: Option<&str>) -> Self {
        Self {
            platform: "app",
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
            device_manufacturer: None,
            device_model: None,
            carrier: "none",
        }
    }

    pub fn with_device(mut self, manufacturer: Option<String>, model: Option<String>) -> Self {
        fn reported(value: Option<String>) -> Option<String> {
            value.map(|value| value.trim().to_owned()).filter(|value| {
                !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
            })
        }
        self.device_manufacturer = reported(manufacturer);
        self.device_model = reported(model);
        self
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new("unknown", None, None)
    }
}
