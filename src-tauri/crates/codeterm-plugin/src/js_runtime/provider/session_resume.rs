use super::JsProvider;
use serde::Deserialize;

#[derive(Deserialize)]
struct SessionResume {
    exact: bool,
}

impl JsProvider {
    pub(super) fn js_supports_exact_session_resume(&self) -> bool {
        super::call_json::<SessionResume>(&self.vm, "sessionResume", &[])
            .is_ok_and(|capability| capability.exact)
    }
}
