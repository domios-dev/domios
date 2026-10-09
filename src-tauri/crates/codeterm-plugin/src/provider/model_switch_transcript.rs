use super::ModelSwitchTranscript;

impl ModelSwitchTranscript {
    pub fn records<'a>(
        &self,
        bytes: &'a str,
    ) -> impl DoubleEndedIterator<Item = Result<serde_json::Value, String>> + 'a {
        bytes
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                serde_json::from_str(line).map_err(|error| {
                    format!("cannot classify the command transcript window: {error}")
                })
            })
    }

    pub fn record_content<'a>(
        &self,
        record: &'a serde_json::Value,
    ) -> Option<&'a serde_json::Value> {
        (record["type"].as_str() == Some(self.record_type.as_str()))
            .then(|| self.content_at_path(record))
            .flatten()
    }

    fn content_at_path<'a>(&self, record: &'a serde_json::Value) -> Option<&'a serde_json::Value> {
        self.content_path
            .iter()
            .try_fold(record, |value, key| value.get(key))
    }

    pub fn stdout<'a>(&self, content: &'a str) -> Option<&'a str> {
        if self.prefix.is_empty() || self.suffix.is_empty() {
            return None;
        }
        content
            .strip_prefix(&self.prefix)
            .and_then(|body| body.strip_suffix(&self.suffix))
            .filter(|body| !body.contains(&self.suffix))
    }

    pub fn record_stdout(&self, record: &serde_json::Value) -> Option<String> {
        let content = match self.record_content(record)? {
            serde_json::Value::String(content) => content.clone(),
            serde_json::Value::Array(blocks) => blocks
                .iter()
                .map(|block| {
                    (block["type"] == "text")
                        .then(|| block["text"].as_str())
                        .flatten()
                })
                .collect::<Option<Vec<_>>>()?
                .join(" "),
            _ => return None,
        };
        self.stdout(&content).map(str::to_owned)
    }
    pub fn has_foreign_user_turn(&self, bytes: &str) -> Result<bool, String> {
        let source = self;
        let ignored = source
            .ignored_content_regexes
            .iter()
            .map(|pattern| regex::Regex::new(pattern))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("invalid command transcript exclusion: {error}"))?;
        for record in self.records(bytes) {
            let record = record?;
            if record["type"].as_str() != Some(source.record_type.as_str())
                && record["type"].as_str() != Some("user")
            {
                continue;
            }
            let content = self.content_at_path(&record);
            let excluded = match content {
                Some(serde_json::Value::String(content)) => {
                    command_content(content, source, &ignored)
                }
                Some(serde_json::Value::Array(blocks)) if !blocks.is_empty() => {
                    blocks.iter().all(|block| {
                        block["type"] == "tool_result"
                            || (block["type"] == "text"
                                && block["text"]
                                    .as_str()
                                    .is_some_and(|text| command_content(text, source, &ignored)))
                    })
                }
                _ => false,
            };
            if !excluded {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn command_content(
    content: &str,
    source: &ModelSwitchTranscript,
    ignored: &[regex::Regex],
) -> bool {
    source.stdout(content).is_some()
        || ignored.iter().any(|pattern| {
            pattern
                .find(content)
                .is_some_and(|found| found.start() == 0 && found.end() == content.len())
        })
}
