use clap::Parser;

use super::*;
use crate::{AgentAction, Cli, Commands, MeshAction, PlanAction};

#[test]
fn parses_mesh_cloud_verbs_and_flags() {
    let create = Cli::try_parse_from([
        "domios", "mesh", "cloud", "create", "--size", "small", "--name", "box-1",
    ])
    .unwrap();
    assert!(matches!(
        create.command,
        Commands::Mesh {
            action: MeshAction::Cloud {
                action: MeshCloudAction::Create {
                    size: Some(ref s),
                    name: Some(ref n),
                }
            }
        } if s == "small" && n == "box-1"
    ));

    let delete =
        Cli::try_parse_from(["domios", "mesh", "cloud", "delete", "cloud-1", "--confirm"]).unwrap();
    assert!(matches!(
        delete.command,
        Commands::Mesh {
            action: MeshAction::Cloud {
                action: MeshCloudAction::Delete {
                    ref machine,
                    confirm: true,
                }
            }
        } if machine == "cloud-1"
    ));

    for verb in ["list", "usage"] {
        let parsed = Cli::try_parse_from(["domios", "mesh", "cloud", verb]).unwrap();
        assert!(matches!(
            parsed.command,
            Commands::Mesh {
                action: MeshAction::Cloud { .. }
            }
        ));
    }

    for verb in ["sleep", "wake"] {
        let parsed = Cli::try_parse_from(["domios", "mesh", "cloud", verb, "cloud-1"]).unwrap();
        assert!(matches!(
            parsed.command,
            Commands::Mesh {
                action: MeshAction::Cloud { .. }
            }
        ));
    }
}

#[test]
fn parses_cloud_flag_on_agent_spawn_and_plan_task_dispatch() {
    let spawn_auto = Cli::try_parse_from(["domios", "agent", "spawn", "codex", "--cloud"]).unwrap();
    assert!(matches!(
        spawn_auto.command,
        Commands::Agent {
            action: AgentAction::Spawn {
                cloud: Some(ref target),
                ..
            }
        } if target == "auto"
    ));

    let spawn_new =
        Cli::try_parse_from(["domios", "agent", "spawn", "codex", "--cloud", "new"]).unwrap();
    assert!(matches!(
        spawn_new.command,
        Commands::Agent {
            action: AgentAction::Spawn {
                cloud: Some(ref target),
                ..
            }
        } if target == "new"
    ));

    let dispatch_auto = Cli::try_parse_from([
        "domios",
        "plan",
        "task",
        "dispatch",
        "pl_1",
        "task_1",
        "--execution-intent",
        "mutating",
        "--cloud",
    ])
    .unwrap();
    assert!(matches!(
        dispatch_auto.command,
        Commands::Plan {
            action: PlanAction::Task {
                action: crate::args::plan::PlanTaskAction::Dispatch {
                    cloud: Some(ref target),
                    ..
                }
            }
        } if target == "auto"
    ));
}

#[test]
fn format_receipt_and_usage_human_surface_managed_cost_fields() {
    let receipt = serde_json::json!({
        "kind": "cloud",
        "machine_id": "cloud-abc",
        "name": "worker-1",
        "provider": "timeweb",
        "size": "small",
        "rate_per_hour": { "amount": 0.20, "currency": "USD", "credits_per_hour": 1.0 },
        "state": "awake",
        "uptime_secs": 3725,
        "billing_notice": "This machine costs money while awake; reuse it before creating another and let it sleep when idle."
    });
    let rendered = format_receipt_human(&receipt);
    assert!(
        rendered.contains("cloud timeweb/small 0.20 USD/h (1.00 cr/h) state=awake uptime=1h02m")
    );
    assert!(rendered.contains("costs money"));

    let usage = serde_json::json!({
        "used": 3.5,
        "allowance": 10.0,
        "purchased": 25.0,
        "remaining": 31.5,
        "cap": 50.0,
        "burn_per_size": { "small": 1.0, "medium": 2.0 }
    });
    let rendered_usage = format_usage_human(&usage);
    assert!(rendered_usage.contains("used=3.50h"));
    assert!(rendered_usage.contains("remaining=31.50h"));
    assert!(rendered_usage.contains("allowance=10.00h"));
    assert!(rendered_usage.contains("purchased=25.00h"));
    assert!(rendered_usage.contains("cap=50.00"));
    assert!(rendered_usage.contains("medium=2.00 cr/h"));
    assert!(rendered_usage.contains("small=1.00 cr/h"));
}
