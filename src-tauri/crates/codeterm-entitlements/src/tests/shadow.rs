use super::support::{admin, free, paid, LocalSigner, T};
use crate::payload::Authority;
use crate::shadow::{shadow, ApplicationAccess, Divergence};
use serde_json::{json, Value};

const PACKET: &str = include_str!("../../vectors/grant-vectors.json");

fn access(fields: Value) -> ApplicationAccess {
    let packet: Value = serde_json::from_str(PACKET).unwrap();
    let paid = packet["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|vector| vector["id"] == "valid-paid")
        .unwrap();
    let mut base = paid["response"]["application_access"].clone();
    super::support::patch(&mut base, fields);
    serde_json::from_value(base).unwrap()
}

#[test]
fn shadow_divergence_table() {
    let signer = LocalSigner::new();
    let paid_grant = signer.accept(
        &super::support::with(
            paid(),
            json!({"perks": {"sponsor": false, "priority_support": false}}),
        ),
        T,
    );
    let admin_grant = signer.accept(&admin(), T);
    let free_grant = signer.accept(&free(), T);
    let free_access = json!({"authority": "free", "max_mesh_machines": 2, "max_workspaces": 2});
    let admin_access = json!({"authority": "admin", "full_access": true, "priority_support": true, "admin_generation": 3});
    let cases = [
        (paid_grant.verdict(T), access(json!({})), vec![]),
        (admin_grant.verdict(T), access(admin_access), vec![]),
        (free_grant.verdict(T), access(free_access.clone()), vec![]),
        (
            free_grant.verdict(T),
            access(json!({})),
            vec![
                Divergence::Authority {
                    grant: Authority::Free,
                    envelope: Authority::Billing,
                },
                Divergence::MeshMachines {
                    grant: Some(2),
                    envelope: None,
                },
                Divergence::Workspaces {
                    grant: Some(2),
                    envelope: None,
                },
            ],
        ),
        (
            paid_grant.verdict(paid_grant.payload().expires_at),
            access(json!({"sponsor": true, "priority_support": true})),
            vec![
                Divergence::Authority {
                    grant: Authority::Free,
                    envelope: Authority::Billing,
                },
                Divergence::MeshMachines {
                    grant: Some(2),
                    envelope: None,
                },
                Divergence::Workspaces {
                    grant: Some(2),
                    envelope: None,
                },
                Divergence::Sponsor {
                    grant: false,
                    envelope: true,
                },
                Divergence::PrioritySupport {
                    grant: false,
                    envelope: true,
                },
            ],
        ),
        (
            paid_grant.verdict(paid_grant.payload().expires_at),
            access(free_access),
            vec![],
        ),
    ];
    for (index, (verdict, envelope, expected)) in cases.into_iter().enumerate() {
        assert_eq!(shadow(&verdict, &envelope), expected, "case {index}");
    }
}
