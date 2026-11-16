pub use codeterm_plugin::token_usage::ParsedUsage;

#[cfg(test)]
pub fn parse_record(value: &serde_json::Value) -> Option<ParsedUsage> {
    use codeterm_plugin::token_usage::{cumulative, grouped, nested};
    let now = super::super::attribution::now_epoch_secs();
    nested::parse_record(value, now)
        .or_else(|| cumulative::parse_record(value, now))
        .or_else(|| grouped::parse_record(value, now))
}
