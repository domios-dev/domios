use super::super::replacement::ReplacementReason;
use super::*;

fn job(id: &str, agent: &str, model: Option<&str>) -> CronJob {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "name": format!("job {id}"),
        "workspace_id": "ws",
        "schedule": "0 3 * * *",
        "enabled": true,
        "agent": agent,
        "model": model,
        "task": "t",
        "created_at": 0,
    }))
    .expect("cron job fixture")
}

fn change(slot: DefaultSlot, provider: &str, from: &str, to: &str) -> PlannedChange {
    PlannedChange {
        stored: StoredDefault {
            slot,
            provider: provider.into(),
            model: from.into(),
            subject: "Default model".into(),
        },
        replacement: Replacement {
            model: to.into(),
            reason: ReplacementReason::SameFamilyLatest,
        },
    }
}

#[test]
fn every_stored_config_default_is_inventoried_with_its_owner() {
    let mut config = AppConfig::default();
    config.general_agent.agent_provider = "codex".into();
    config
        .agents
        .entry("codex".into())
        .or_default()
        .default_model = Some("gpt-5.1-codex".into());
    config
        .agents
        .entry("claude".into())
        .or_default()
        .default_model = Some(" ".into());
    config.memory.extraction_agent = "claude".into();
    config.memory.extraction_model = "haiku".into();
    config.memory.dreaming_agent = String::new();
    config.memory.dreaming_model = String::new();
    config.memory.worker_model = "gpt-5.5".into();

    let defaults = config_defaults(&config);
    let owners: Vec<(String, &str, &str)> = defaults
        .iter()
        .map(|stored| {
            (
                stored.slot.key(),
                stored.provider.as_str(),
                stored.model.as_str(),
            )
        })
        .collect();
    assert_eq!(
        owners,
        vec![
            (
                "agents.codex.default_model".to_string(),
                "codex",
                "gpt-5.1-codex"
            ),
            ("memory.extraction_model".to_string(), "claude", "haiku"),
            ("memory.worker_model".to_string(), "codex", "gpt-5.5"),
        ]
    );
}

#[test]
fn worker_model_is_skipped_when_both_workers_name_their_own_model() {
    let mut config = AppConfig::default();
    config.memory.extraction_model = "haiku".into();
    config.memory.dreaming_model = "sonnet".into();
    config.memory.worker_model = "haiku".into();
    assert!(config_defaults(&config)
        .iter()
        .all(|stored| stored.slot != DefaultSlot::MemoryWorker));
}

#[test]
fn cron_jobs_with_a_model_are_inventoried() {
    let jobs = [
        job("a", "codex", Some("gpt-5.1-codex")),
        job("b", "claude", None),
    ];
    let defaults = cron_defaults(&jobs);
    assert_eq!(defaults.len(), 1);
    assert_eq!(defaults[0].slot, DefaultSlot::CronJob("a".into()));
    assert_eq!(defaults[0].provider, "codex");
}

#[test]
fn applying_a_change_rewrites_the_slot_and_records_the_note() {
    let mut config = AppConfig::default();
    let planned = change(
        DefaultSlot::ProviderDefault("codex".into()),
        "codex",
        "gpt-5.1-codex",
        "gpt-5.3-codex",
    );
    assert!(apply_to_config(&mut config, &planned));
    assert_eq!(
        config.agents["codex"].default_model.as_deref(),
        Some("gpt-5.3-codex")
    );
    let note = &config.model_defaults.replaced["agents.codex.default_model"];
    assert_eq!(
        (note.from.as_str(), note.to.as_str()),
        ("gpt-5.1-codex", "gpt-5.3-codex")
    );

    let memory = change(
        DefaultSlot::MemoryDreaming,
        "claude",
        "sonnet-old",
        "sonnet",
    );
    assert!(apply_to_config(&mut config, &memory));
    assert_eq!(config.memory.dreaming_model, "sonnet");
}

#[test]
fn cron_slots_are_not_config_owned() {
    let mut config = AppConfig::default();
    let planned = change(DefaultSlot::CronJob("a".into()), "codex", "old", "new");
    assert!(!apply_to_config(&mut config, &planned));
    assert!(config.model_defaults.replaced.is_empty());
}

#[test]
fn notice_lists_every_change_once() {
    let changes = [
        change(
            DefaultSlot::ProviderDefault("codex".into()),
            "codex",
            "old-1",
            "new-1",
        ),
        change(DefaultSlot::MemoryWorker, "claude", "old-2", "new-2"),
    ];
    let body = notice_body(&changes, |id| id.to_uppercase());
    assert_eq!(body.lines().count(), 2);
    for value in ["CODEX", "old-1", "new-1", "CLAUDE", "old-2", "new-2"] {
        assert!(body.contains(value), "{value} missing from {body}");
    }
}
