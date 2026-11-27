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
                    recoverable: false,
                    archive_id: None,
                }
            }
        } if machine == "cloud-1"
    ));

    let delete_rec = Cli::try_parse_from([
        "domios",
        "mesh",
        "cloud",
        "delete",
        "cloud-1",
        "--confirm",
        "--recoverable",
        "--archive-id",
        "arch-1",
    ])
    .unwrap();
    assert!(matches!(
        delete_rec.command,
        Commands::Mesh {
            action: MeshAction::Cloud {
                action: MeshCloudAction::Delete {
                    ref machine,
                    confirm: true,
                    recoverable: true,
                    archive_id: Some(ref aid),
                }
            }
        } if machine == "cloud-1" && aid == "arch-1"
    ));

    let recreate = Cli::try_parse_from([
        "domios",
        "mesh",
        "cloud",
        "recreate",
        "cloud-1",
        "--archive-id",
        "arch-1",
    ])
    .unwrap();
    assert!(matches!(
        recreate.command,
        Commands::Mesh {
            action: MeshAction::Cloud {
                action: MeshCloudAction::Recreate {
                    ref machine,
                    archive_id: Some(ref aid),
                }
            }
        } if machine == "cloud-1" && aid == "arch-1"
    ));

    let resize = Cli::try_parse_from([
        "domios",
        "mesh",
        "cloud",
        "resize-volume",
        "cloud-1",
        "--size-gb",
        "20",
        "--confirm",
    ])
    .unwrap();
    assert!(matches!(
        resize.command,
        Commands::Mesh {
            action: MeshAction::Cloud {
                action: MeshCloudAction::ResizeVolume {
                    ref machine,
                    size_gb: 20,
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

    for verb in ["sleep", "archive", "wake"] {
        let parsed = Cli::try_parse_from(["domios", "mesh", "cloud", verb, "cloud-1"]).unwrap();
        assert!(matches!(
            parsed.command,
            Commands::Mesh {
                action: MeshAction::Cloud { .. }
            }
        ));
    }

    let redeem = Cli::try_parse_from(["domios", "mesh", "cloud", "redeem", "BETA-25"]).unwrap();
    assert!(matches!(
        redeem.command,
        Commands::Mesh {
            action: MeshAction::Cloud {
                action: MeshCloudAction::Redeem { ref code }
            }
        } if code == "BETA-25"
    ));

    let topup_pack =
        Cli::try_parse_from(["domios", "mesh", "cloud", "topup", "--pack", "pack_5"]).unwrap();
    assert!(matches!(
        topup_pack.command,
        Commands::Mesh {
            action: MeshAction::Cloud {
                action: MeshCloudAction::Topup {
                    pack: Some(ref p),
                    amount: None,
                }
            }
        } if p == "pack_5"
    ));

    let topup_amount =
        Cli::try_parse_from(["domios", "mesh", "cloud", "topup", "--amount", "7"]).unwrap();
    assert!(matches!(
        topup_amount.command,
        Commands::Mesh {
            action: MeshAction::Cloud {
                action: MeshCloudAction::Topup {
                    pack: None,
                    amount: Some(ref a),
                }
            }
        } if a == "7"
    ));

    assert!(Cli::try_parse_from(["domios", "mesh", "cloud", "topup"]).is_err());
    assert!(Cli::try_parse_from([
        "domios", "mesh", "cloud", "topup", "--pack", "pack_5", "--amount", "7"
    ])
    .is_err());
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

    let redeemed = serde_json::json!({
        "schema_version": 1,
        "status": "redeemed",
        "already_redeemed": false,
        "hours": 25.0,
        "expires_at": 1_800_000_000u64,
        "remaining": 35.0,
        "purchased": 25.0
    });
    let rendered_redeem = format_redeem_human(&redeemed);
    assert!(rendered_redeem.contains("Redeemed promo code: +25.00h"));
    assert!(rendered_redeem.contains("expires_at=1800000000"));
    assert!(rendered_redeem.contains("remaining=35.00h"));

    let already = serde_json::json!({
        "schema_version": 1,
        "status": "redeemed",
        "already_redeemed": true,
        "hours": 25.0,
        "expires_at": null,
        "remaining": 35.0,
        "purchased": 25.0
    });
    let rendered_already = format_redeem_human(&already);
    assert!(rendered_already.contains("already redeemed"));
    assert!(rendered_already.contains("expires=never"));

    let checkout = serde_json::json!({
        "url": "https://checkout.stripe.com/c/pay/cs_test_fixture_pack_5",
        "session_id": "cs_test_fixture_pack_5",
        "pack_id": "pack_5",
        "credits": 25.0,
        "amount_cents": 500,
        "currency": "USD"
    });
    assert_eq!(
        format_topup_human(&checkout),
        "https://checkout.stripe.com/c/pay/cs_test_fixture_pack_5"
    );
}

#[test]
fn parses_topup_usd_amounts_to_exact_cents() {
    assert_eq!(parse_usd_to_cents("7").unwrap(), 700);
    assert_eq!(parse_usd_to_cents("$7").unwrap(), 700);
    assert_eq!(parse_usd_to_cents("7.00").unwrap(), 700);
    assert_eq!(parse_usd_to_cents("12.5").unwrap(), 1250);
    assert_eq!(parse_usd_to_cents("12.50").unwrap(), 1250);
    for bad in ["", " ", "0", "0.00", "-5", "+5", "7.", "7.123", "abc", "$"] {
        assert!(parse_usd_to_cents(bad).is_err(), "{bad:?}");
    }
}
