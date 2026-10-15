use serde::Deserialize;

#[derive(Clone, Default)]
pub struct Selection {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub resume: Option<String>,
    pub extra_args: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SendBatchItem {
    pub text: String,
    pub delay_ms: Option<u64>,
}

pub enum Input<'a> {
    Text(&'a str),
    TextDetached(&'a str),
    Insert(&'a str),
    Raw(&'a [u8]),
    Batch(Vec<SendBatchItem>),
}
