use crate::blueprint_github::{GithubProbe, GithubResource};
use serde_json::{json, Value};

pub struct SourceReading {
    pub state: String,
    pub summary: String,
    pub output: Value,
}

pub fn github_reading(
    probe: &GithubProbe,
    mut value: Value,
) -> Result<Option<SourceReading>, String> {
    if probe.resource == GithubResource::PrChecks {
        return Ok(None);
    }
    if probe.resource == GithubResource::Runners {
        let pages = match &value {
            Value::Array(pages) => pages.clone(),
            Value::Object(_) => vec![value.clone()],
            _ => return Err("invalid GitHub runners response".into()),
        };
        let mut online = 0;
        for page in pages {
            let runners = page
                .get("runners")
                .and_then(Value::as_array)
                .ok_or("GitHub runners response has no runners array")?;
            for runner in runners {
                if !matches!(runner["status"].as_str(), Some("online" | "offline"))
                    || !runner["labels"].is_array()
                {
                    return Err("invalid GitHub runner status or labels".into());
                }
                let matches = probe.label.as_ref().is_none_or(|label| {
                    label == "*"
                        || runner
                            .get("labels")
                            .and_then(Value::as_array)
                            .is_some_and(|labels| {
                                labels.iter().any(|entry| {
                                    entry["name"]
                                        .as_str()
                                        .is_some_and(|name| name.eq_ignore_ascii_case(label))
                                })
                            })
                });
                if matches && runner["status"] == "online" {
                    online += 1;
                }
            }
        }
        let state = if online > 0 { "online" } else { "offline" };
        return Ok(Some(SourceReading {
            state: state.into(),
            summary: format!("{state} · {online} online"),
            output: json!({"state":state,"online_count":online,"label":probe.label}),
        }));
    }
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .ok_or("GitHub run response has no status")?
        .to_string();
    let jobs = value
        .get("jobs")
        .and_then(Value::as_array)
        .ok_or("GitHub run response has no jobs array")?;
    let failed_jobs: Vec<_> = jobs
        .iter()
        .filter(|job| matches!(job["conclusion"].as_str(), Some("failure" | "timed_out")))
        .filter_map(|job| job["name"].as_str())
        .collect();
    let failing = status != "completed" && !failed_jobs.is_empty();
    let names = failed_jobs.join(", ");
    let state = if failing { "failing" } else { "done" };
    let summary = if failing {
        format!("failing · {names}")
    } else {
        format!(
            "{status} · {}",
            value["conclusion"].as_str().unwrap_or("pending")
        )
    };
    let failed_jobs = failed_jobs
        .iter()
        .map(|name| Value::String((*name).into()))
        .collect::<Vec<_>>();
    value["source_state"] = state.into();
    value["failed_jobs"] = failed_jobs.into();
    Ok(Some(SourceReading {
        state: state.into(),
        summary,
        output: value,
    }))
}

#[cfg(test)]
mod tests;
