use serde_json::Value;

pub(super) struct Arguments<'a> {
    pub json: String,
    pub raw: Option<&'a str>,
    pub objects: Option<&'a [Value]>,
}

impl<'a> Arguments<'a> {
    pub fn prepare(method: &str, args: &'a [Value]) -> Result<Self, serde_json::Error> {
        if matches!(
            (method, args),
            (
                "parseSessionRecords" | "parseSessionMessageRecords",
                [Value::Array(_), Value::Object(_)]
            ) | (
                "runtimeStoredRecords",
                [Value::String(_), Value::Array(_), Value::Object(_)]
            )
        ) {
            return Ok(Self {
                json: String::new(),
                raw: None,
                objects: Some(args),
            });
        }
        let raw = match (method, args) {
            ("runtimeStoredHistory", [Value::String(_), Value::String(records)]) => {
                Some(records.as_str())
            }
            _ => None,
        };
        let envelope = if raw.is_some() { &args[..1] } else { args };
        Ok(Self {
            json: serde_json::to_string(envelope)?,
            raw,
            objects: None,
        })
    }

    pub fn bytes(&self) -> usize {
        self.json.len() + self.raw.map_or(0, str::len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stored_history_selects_only_its_two_string_shape() {
        let args = [json!("session"), json!("records")];
        let prepared = Arguments::prepare("runtimeStoredHistory", &args).unwrap();
        assert_eq!(prepared.json, r#"["session"]"#);
        assert_eq!(prepared.raw, Some("records"));
        for (method, args) in [
            ("echo", args.to_vec()),
            ("runtimeStoredHistory", vec![json!("records")]),
            ("runtimeStoredHistory", vec![json!(null), json!("records")]),
            ("runtimeStoredHistory", vec![json!("session"), json!({})]),
            (
                "runtimeStoredHistory",
                vec![json!("session"), json!("records"), json!(0)],
            ),
        ] {
            let prepared = Arguments::prepare(method, &args).unwrap();
            assert_eq!(prepared.raw, None);
            assert_eq!(
                serde_json::from_str::<Vec<Value>>(&prepared.json).unwrap(),
                args
            );
        }
    }

    #[test]
    fn raw_string_is_borrowed_without_json_encoding() {
        let args = [json!("session"), json!("\"\n\\\0🦀")];
        let prepared = Arguments::prepare("runtimeStoredHistory", &args).unwrap();
        let raw = prepared.raw.unwrap();
        let original = args[1].as_str().unwrap();
        assert_eq!(raw.as_ptr(), original.as_ptr());
        assert_eq!(raw.as_bytes(), original.as_bytes());
    }
}
