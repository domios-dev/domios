use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LocalRecordingTrigger {
    pub vendor_id: u16,
    pub product_id: u16,
    pub usage_page: u16,
    pub usage: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub button: Option<LocalRecordingButton>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LocalRecordingButton {
    pub interface_number: u8,
    pub collection_id: String,
    pub report_id: u8,
    pub report_bytes: u16,
    pub usage_page: u16,
    pub press_usage: u16,
    pub button_usages: Vec<u16>,
    pub bit_field: u16,
    pub link_collection: u16,
    pub debounce_ms: u16,
}

impl LocalRecordingButton {
    pub fn valid(&self) -> bool {
        let name = self.collection_id.as_bytes();
        name.len() == 5
            && name[..3].eq_ignore_ascii_case(b"col")
            && name[3..].iter().all(u8::is_ascii_hexdigit)
            && self.report_id != 0
            && (2..=64).contains(&self.report_bytes)
            && self.usage_page != 0
            && !self.button_usages.is_empty()
            && self.button_usages.len() <= 64
            && self.button_usages.contains(&self.press_usage)
            && self
                .button_usages
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == self.button_usages.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReceiverStatusQuery {
    pub usage_page: u16,
    pub usage: u16,
    pub report_id: u8,
    pub request: Vec<u8>,
}

impl ReceiverStatusQuery {
    pub fn valid(&self) -> bool {
        (2..=64).contains(&self.request.len())
            && self.report_id != 0
            && self.request.first() == Some(&self.report_id)
    }
}

#[cfg(test)]
mod tests;
