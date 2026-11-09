use super::JsProvider;
use serde::Deserialize;

#[derive(Deserialize, ts_rs::TS)]
struct SessionResume {
    exact: bool,
}

impl JsProvider {
    pub(super) fn js_supports_exact_session_resume(&self) -> bool {
        super::call_json::<SessionResume>(&self.vm, "sessionResume", &[])
            .is_ok_and(|capability| capability.exact)
    }
}

pub(super) fn export_protocol_contracts(
    config: &ts_rs::Config,
) -> Result<(), Box<dyn std::error::Error>> {
    use ts_rs::TS;
    SessionResume::export_all(config)?;
    Ok(())
}
