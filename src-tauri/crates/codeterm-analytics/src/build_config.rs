use crate::Project;

pub const AMPLITUDE_TEST_KEY_FINGERPRINT: &str = concat!(
    "domios-amplitude-test:",
    env!("DOMIOS_AMPLITUDE_FINGERPRINT_TEST")
);
pub const AMPLITUDE_PROD_KEY_FINGERPRINT: &str = concat!(
    "domios-amplitude-prod:",
    env!("DOMIOS_AMPLITUDE_FINGERPRINT_PROD")
);

pub(crate) fn amplitude_key(project: Project) -> Option<&'static str> {
    match project {
        Project::Test => option_env!("DOMIOS_AMPLITUDE_API_KEY_TEST"),
        Project::Prod => option_env!("DOMIOS_AMPLITUDE_API_KEY_PROD"),
    }
    .filter(|value| !value.trim().is_empty())
}
