use serde::Deserialize;

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "GrabRect")]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, Deserialize, ts_rs::TS)]
#[ts(rename = "GrabPageFacts")]
pub struct PageFacts {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub viewport: String,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, Deserialize, ts_rs::TS)]
#[ts(rename = "GrabAccessibility")]
pub struct Accessibility {
    pub role: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GrabTarget {
    pub tag_name: String,
    #[serde(default)]
    pub selector: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub classes: String,
    #[serde(default)]
    #[serde(serialize_with = "serialize_pairs")]
    #[ts(as = "std::collections::BTreeMap<String, String>")]
    pub attributes: Vec<(String, String)>,
    #[serde(default)]
    #[serde(serialize_with = "serialize_pairs")]
    #[ts(as = "std::collections::BTreeMap<String, String>")]
    pub styles: Vec<(String, String)>,
    #[serde(default)]
    pub react: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub nearby_text: Vec<String>,
    #[serde(default)]
    pub ancestor_path: Vec<String>,
    #[serde(default)]
    pub rect_page: Rect,
    #[serde(default)]
    pub page: PageFacts,
    #[serde(skip_deserializing)]
    pub accessibility: Accessibility,
}

fn serialize_pairs<S: serde::Serializer>(
    pairs: &[(String, String)],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::Serialize;
    pairs
        .iter()
        .cloned()
        .collect::<std::collections::BTreeMap<_, _>>()
        .serialize(serializer)
}

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
pub struct GrabSelection {
    pub id: String,
    pub label: String,
    pub target: GrabTarget,
}

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
pub struct SnipCapture {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub thumbnail: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
pub struct ProfilesResult {
    pub current: String,
    pub profiles: Vec<String>,
}

#[cfg(test)]
mod tests;
