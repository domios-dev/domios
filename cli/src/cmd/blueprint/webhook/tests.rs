use super::*;
use crate::args::cli::{Cli, Commands};
use clap::Parser;

#[test]
fn webhook_lifecycle_requires_explicit_reveal_and_rejects_path_injection() {
    for operation in ["enable", "rotate", "disable", "show-url"] {
        let cli = Cli::try_parse_from(["domios", "blueprint", "webhook", operation, "bp_opaque"])
            .unwrap();
        let Commands::Blueprint {
            action: crate::args::blueprint::BlueprintAction::Webhook { action },
        } = cli.command
        else {
            panic!("webhook command expected");
        };
        let (id, body) = request(&action).unwrap();
        assert_eq!(id, "bp_opaque");
        assert_eq!(body["reveal"], false);
        assert!(body.get("secret").is_none());
    }
    assert!(request(&BlueprintWebhookAction::Disable {
        id: "../other".into()
    })
    .is_err());
    assert_eq!(
        request(&BlueprintWebhookAction::Rotate {
            id: "opaque".into(),
            reveal: true
        })
        .unwrap()
        .1["reveal"],
        true
    );
}
