pub fn validate_sentinel_name(name: &str) -> Result<(), String> {
    validate(
        name,
        "Blueprint graph",
        "describe what is watched and what happens",
        "sentinel",
    )
}

pub fn validate_cron_name(name: &str) -> Result<(), String> {
    validate(name, "Cron job", "describe what the job does", "cron")
}

fn validate(name: &str, kind: &str, meaning: &str, id_prefix: &str) -> Result<(), String> {
    let length = name.chars().count();
    if !(3..=40).contains(&length)
        || name.trim() != name
        || name.chars().any(char::is_control)
        || !name.chars().any(char::is_alphabetic)
        || is_raw_id(name, id_prefix)
        || (id_prefix == "sentinel" && is_raw_id(name, "blueprint"))
    {
        return Err(format!(
            "{kind} name must be 3–40 characters, {meaning}, and not be a raw ID; examples: ci-green-watch, build-done-notify"
        ));
    }
    Ok(())
}

fn is_raw_id(name: &str, prefix: &str) -> bool {
    let Some((candidate, suffix)) = name.split_once('_') else {
        return false;
    };
    candidate.eq_ignore_ascii_case(prefix)
        && suffix.len() >= 8
        && suffix
            .chars()
            .all(|character| character.is_ascii_hexdigit())
}

pub fn generated_handler_id(
    kind: &str,
    target: Option<&str>,
    label: Option<&str>,
    bucket: &str,
    ordinal: usize,
) -> String {
    let action = match kind {
        "notify" => "notify".to_string(),
        "stop" => "stop".to_string(),
        "send" => format!("send-to-{}", slug(target.unwrap_or("agent"), 20)),
        "cron" => format!("run-{}", slug(target.unwrap_or("cron"), 24)),
        "blueprint" | "sentinel" => format!("activate-{}", slug(target.unwrap_or("stage"), 20)),
        "respond_prompt" => format!("respond-to-{}", slug(target.unwrap_or("prompt"), 20)),
        "escalate" => format!(
            "escalate-to-{}",
            if target == Some("creator") {
                "owner".into()
            } else {
                slug(target.unwrap_or("agent"), 20)
            }
        ),
        "command" => format!("run-{}", slug(label.unwrap_or("command"), 24)),
        _ => slug(kind, 16),
    };
    let suffix = if ordinal > 1 {
        format!("-{ordinal}")
    } else {
        String::new()
    };
    let mut id = format!("{action}-on-{}", slug(bucket, 24));
    id.truncate(64usize.saturating_sub(suffix.len()));
    id = id.trim_end_matches('-').to_string();
    id.push_str(&suffix);
    id
}

fn slug(value: &str, max: usize) -> String {
    let mut slug = String::new();
    let mut separator = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if separator && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character.to_ascii_lowercase());
            separator = false;
        } else {
            separator = true;
        }
        if slug.len() >= max {
            break;
        }
    }
    let slug = slug.trim_end_matches('-').to_string();
    if slug.is_empty() {
        "item".into()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::{generated_handler_id, validate_cron_name, validate_sentinel_name};

    #[test]
    fn automation_names_are_bounded_and_reject_raw_ids() {
        assert!(validate_sentinel_name("ci-green-watch").is_ok());
        assert!(validate_sentinel_name("sentinel_5d22889a8898").is_err());
        assert!(validate_sentinel_name("!!!").is_err());
        assert!(validate_sentinel_name("ab").is_err());
        assert!(validate_cron_name("build-done-notify").is_ok());
        assert!(validate_cron_name("cron_5d22889a8898").is_err());
    }

    #[test]
    fn generated_handler_ids_describe_type_target_and_bucket() {
        assert_eq!(
            generated_handler_id("send", Some("Review"), None, "passed", 1),
            "send-to-review-on-passed"
        );
        assert_eq!(
            generated_handler_id("notify", None, None, "ready", 1),
            "notify-on-ready"
        );
        assert_eq!(
            generated_handler_id("notify", None, None, "ready", 2),
            "notify-on-ready-2"
        );
    }
}
