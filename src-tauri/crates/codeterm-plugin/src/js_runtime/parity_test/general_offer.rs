use super::*;

struct NativeGeneralOffer {
    offer: bool,
}

#[async_trait::async_trait]
impl AgentProvider for NativeGeneralOffer {
    fn id(&self) -> &str {
        "general-offer-fixture"
    }

    fn display_name(&self) -> &str {
        "General Offer Fixture"
    }

    fn general_agent_offer_when_absent(&self) -> bool {
        self.offer
    }

    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }

    fn readiness_strategy(&self) -> crate::provider::ReadinessStrategy {
        crate::provider::ReadinessStrategy::FixedDelay {
            duration: std::time::Duration::ZERO,
        }
    }

    fn task_delivery(&self) -> crate::provider::TaskDelivery {
        crate::provider::TaskDelivery::PtyInput
    }

    fn detect_from_title(&self, _: &str) -> bool {
        false
    }

    fn detect_from_output(&self, _: &str) -> bool {
        false
    }

    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }

    fn detect_session_id(
        &self,
        _: &str,
        _: &[String],
        _: Option<std::time::Duration>,
    ) -> Option<String> {
        None
    }

    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
}

#[test]
fn general_offer_native_js_parity_and_default() {
    for offer in [None, Some(false), Some(true)] {
        let mut value = serde_json::json!({
            "id":"general-offer-fixture", "version":"0.1.0", "displayName":"Fixture"
        });
        if let Some(offer) = offer {
            value["generalAgentOfferWhenAbsent"] = offer.into();
        }
        let js = load_inline_js_with_manifest(
            "general-offer-fixture",
            &value.to_string(),
            "module.exports.default = {};",
        );
        let native = NativeGeneralOffer {
            offer: offer.unwrap_or(false),
        };
        assert_eq!(
            js.general_agent_offer_when_absent(),
            native.general_agent_offer_when_absent()
        );
        assert_eq!(
            js.describe().general_agent_offer_when_absent,
            native.describe().general_agent_offer_when_absent
        );
        let wire = serde_json::to_value(js.describe()).unwrap();
        assert_eq!(
            wire.get("general_agent_offer_when_absent"),
            offer
                .filter(|value| *value)
                .map(|_| serde_json::Value::Bool(true))
                .as_ref()
        );
    }
}
