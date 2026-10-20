use serde_json::value::RawValue;
use std::{collections::BTreeMap, io::Write};

type Fields<'a> = BTreeMap<&'a str, &'a RawValue>;

struct BoundedOutput {
    bytes: Vec<u8>,
    limit: usize,
}

impl Write for BoundedOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other(
                "Stored history metadata exceeds page budget",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn oversized_record(record: &str, budget: usize) -> Result<String, String> {
    let project = || -> Result<String, Box<dyn std::error::Error>> {
        let mut envelope: Fields<'_> = serde_json::from_str(record)?;
        let mut payload: Fields<'_> = serde_json::from_str(
            envelope
                .get("payload")
                .ok_or("Missing stored record payload")?
                .get(),
        )?;
        let mut item: Fields<'_> = serde_json::from_str(
            payload
                .get("item")
                .ok_or("Missing stored history item")?
                .get(),
        )?;
        item.retain(|key, _| matches!(*key, "id" | "type" | "status" | "startedAt" | "started_at"));
        item.insert("historyTruncated", serde_json::from_str("true")?);
        let item = bounded_json(&item, budget)?;
        let item: &RawValue = serde_json::from_str(&item)?;
        payload.insert("item", item);
        let payload = bounded_json(&payload, budget)?;
        let payload: &RawValue = serde_json::from_str(&payload)?;
        envelope.insert("payload", payload);
        let mut result = bounded_json(&envelope, budget.saturating_sub(1))?;
        result.push('\n');
        Ok(result)
    };
    project().map_err(|error| format!("Cannot project oversized stored history record: {error}"))
}

fn bounded_json(value: &impl serde::Serialize, budget: usize) -> Result<String, serde_json::Error> {
    let mut output = BoundedOutput {
        bytes: Vec::new(),
        limit: budget,
    };
    serde_json::to_writer(&mut output, value)?;
    Ok(String::from_utf8(output.bytes).expect("JSON serialization produces UTF-8"))
}
