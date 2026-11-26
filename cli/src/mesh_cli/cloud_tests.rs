use clap::Parser;

use super::*;
use crate::{AgentAction, Cli, Commands, MeshAction, PlanAction};

#[test]
fn parses_mesh_cloud_verbs_and_flags() {
    let create = Cli::try_parse_from([
        "domios",
        "mesh",
        "cloud",
        "create",
        "--provider",
        "daytona",
        "--size",
        "small",
        "--name",
        "box-1",
    ])
    .unwrap();
    assert!(matches!(
        create.command,
        Commands::Mesh {
            action: MeshAction::Cloud {
                action: MeshCloudAction::Create {
                    provider: Some(ref p),
                    size: Some(ref s),
                    name: Some(ref n),
                }
            }
        } if p == "daytona" && s == "small" && n == "box-1"
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
fn format_receipt_human_surfaces_cost_fields_and_warning() {
    let receipt = serde_json::json!({
        "kind": "cloud",
        "machine_id": "cloud-abc",
        "name": "worker-1",
        "provider": "daytona",
        "size": "small",
        "rate_per_hour": { "amount": 0.08, "currency": "USD", "basis": "estimate" },
        "state": "awake",
        "uptime_secs": 3725,
        "billing_notice": "This machine costs money while awake; reuse it before creating another and let it sleep when idle."
    });
    let rendered = format_receipt_human(&receipt);
    assert!(rendered.contains("cloud daytona/small 0.08 USD/h (estimate) state=awake uptime=1h02m"));
    assert!(rendered.contains("costs money"));
}
