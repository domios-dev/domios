use crate::Project;
pub(crate) fn amplitude_key(project: Project) -> Option<&'static str> {
    match project {
        Project::Test => option_env!("DOMIOS_AMPLITUDE_API_KEY_TEST"),
        Project::Prod => option_env!("DOMIOS_AMPLITUDE_API_KEY_PROD"),
    }
    .filter(|value| !value.trim().is_empty())
}
