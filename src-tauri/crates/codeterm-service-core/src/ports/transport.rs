use serde::Deserialize;

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
