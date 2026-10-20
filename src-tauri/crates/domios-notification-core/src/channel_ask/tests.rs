use super::*;
use crate::authority::{adopt_offer, SurfaceTarget};
use crate::contract::{answer_actions, validate_answer_labels, CallerAction};

const CHOSEN: &str = "Rebase";
const DECLINED: &str = "Merge";

fn choice_set(labels: &[&str]) -> Vec<CallerAction> {
    answer_actions(&labels.iter().map(|l| l.to_string()).collect::<Vec<_>>())
        .expect("distinct non-empty labels are a legal choice set")
}

/// The choice set of an ask-notification is vetted by the one shared
/// validator, on the offer path exactly as on every other. Nothing here
/// restates what a refusable label is.
#[test]
fn channel_ask_labels_are_refused_by_the_existing_validation() {
    for unusable in [
        vec!["Rebase".to_string(), "  ".to_string()],
        vec!["Rebase".to_string(), String::new()],
        vec!["Rebase".to_string(), "rebase".to_string()],
        vec!["Rebase".to_string(), " REBASE ".to_string()],
    ] {
        let built = answer_actions(&unusable);
        assert!(
            built.is_err(),
            "{unusable:?} is not a choice set a user could tell apart"
        );
        // The very same refusal the shared validator produces for the same
        // input: this path reuses it rather than carrying a copy of the rule.
        let vetted: Vec<CallerAction> = unusable
            .iter()
            .map(|label| CallerAction::Answer {
                label: label.trim().to_string(),
            })
            .collect();
        assert_eq!(
            built.unwrap_err(),
            validate_answer_labels(&vetted).expect_err("the shared validator refuses the same set"),
            "the offer path refuses with the shared validator's own answer"
        );
    }

    // A legal set stays legal, and its offered positions are what a channel
    // addresses — so a token cannot name a label that was never offered.
    let legal = choice_set(&[CHOSEN, DECLINED]);
    assert!(validate_answer_labels(&legal).is_ok());
    assert_eq!(offered_labels(&legal), vec![CHOSEN, DECLINED]);
    assert_eq!(label_at(&legal, 1), Some(DECLINED));
    assert_eq!(label_at(&legal, 2), None);

    // And an offer cannot ride a notification that offers no choice at all.
    assert!(adopt_offer(SurfaceTarget::Desktop, CHOSEN, &[] as &[CallerAction]).is_err());
}
