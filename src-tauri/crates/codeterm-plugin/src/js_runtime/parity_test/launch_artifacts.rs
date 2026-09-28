use super::*;
use crate::provider::{
    LaunchArtifactSweepReport, LaunchArtifactSweepRequest, ReadinessStrategy, TaskDelivery,
};

struct LaunchArtifactsNativeStub;

#[async_trait::async_trait]
impl AgentProvider for LaunchArtifactsNativeStub {
    fn id(&self) -> &str {
        "artifact-fixture"
    }

    fn display_name(&self) -> &str {
        "Artifact Fixture"
    }

    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }

    fn session_file_path(&self, _: &str, _: &str) -> Option<std::path::PathBuf> {
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

    fn release_tab_launch_artifacts(
        &self,
        tab_id: &str,
        instance_id: &str,
    ) -> Result<bool, String> {
        Ok(tab_id == "closed-tab" && instance_id == "instance-fixture")
    }

    fn folder_trust_answered(&self, tab_id: &str, instance_id: &str) -> Result<bool, String> {
        Ok(tab_id == "trusted-tab" && instance_id == "instance-fixture")
    }

    fn is_directory_trusted(&self, path: &str) -> Result<bool, String> {
        Ok(path.eq_ignore_ascii_case(r"C:\Trust\Exact"))
    }

    fn sweep_launch_artifacts(
        &self,
        request: &LaunchArtifactSweepRequest,
    ) -> Result<Option<LaunchArtifactSweepReport>, String> {
        let dead = (!request.live_tab_ids.iter().any(|id| id == "dead-tab"))
            .then(|| "dead-tab".to_string())
            .into_iter()
            .collect::<Vec<_>>();
        let legacy = vec!["legacy-tab".to_string()];
        let removed = if request.confirm {
            dead.iter()
                .cloned()
                .chain(
                    request
                        .include_legacy
                        .then(|| legacy.clone())
                        .into_iter()
                        .flatten(),
                )
                .collect()
        } else {
            Vec::new()
        };
        Ok(Some(LaunchArtifactSweepReport {
            dead,
            legacy,
            removed,
        }))
    }

    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: std::time::Duration::ZERO,
        }
    }

    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
    }

    fn detect_from_title(&self, _: &str) -> bool {
        false
    }

    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
}

#[test]
fn launch_artifact_and_directory_trust_native_js_parity() {
    let native = LaunchArtifactsNativeStub;
    let js = load_inline_js(
        "artifact-fixture",
        r#"
        module.exports.default = {
            releaseTabLaunchArtifacts(tabId, instanceId) {
                return tabId === "closed-tab" && instanceId === "instance-fixture";
            },
            folderTrustAnswered(tabId, instanceId) {
                return tabId === "trusted-tab" && instanceId === "instance-fixture";
            },
            isDirectoryTrusted(path) {
                return path.toLowerCase() === "c:\\trust\\exact";
            },
            sweepLaunchArtifacts(request) {
                const dead = request.liveTabIds.includes("dead-tab") ? [] : ["dead-tab"];
                const legacy = ["legacy-tab"];
                const removed = request.confirm
                    ? dead.concat(request.includeLegacy ? legacy : [])
                    : [];
                return { dead, legacy, removed };
            }
        };
        "#,
    );
    let request = LaunchArtifactSweepRequest {
        instance_id: "instance-fixture".into(),
        live_tab_ids: vec!["live-tab".into()],
        confirm: true,
        include_legacy: true,
    };

    assert_eq!(
        native.release_tab_launch_artifacts("closed-tab", &request.instance_id),
        js.release_tab_launch_artifacts("closed-tab", &request.instance_id),
    );
    assert_eq!(
        native.folder_trust_answered("trusted-tab", &request.instance_id),
        js.folder_trust_answered("trusted-tab", &request.instance_id),
    );
    assert_eq!(
        native.is_directory_trusted(r"C:\Trust\Exact"),
        js.is_directory_trusted(r"C:\Trust\Exact"),
    );
    assert_eq!(
        native.is_directory_trusted(r"C:\Trust\Other"),
        js.is_directory_trusted(r"C:\Trust\Other"),
    );
    assert_eq!(
        native.sweep_launch_artifacts(&request),
        js.sweep_launch_artifacts(&request),
    );
}

#[test]
fn launch_artifact_js_errors_reach_the_caller() {
    let js = load_inline_js(
        "artifact-error-fixture",
        r#"
        module.exports.default = {
            releaseTabLaunchArtifacts() { throw new Error("release fixture failure"); },
            sweepLaunchArtifacts() { throw new Error("sweep fixture failure"); }
        };
        "#,
    );
    let request = LaunchArtifactSweepRequest::default();

    assert!(js
        .release_tab_launch_artifacts("tab", "instance")
        .unwrap_err()
        .contains("release fixture failure"));
    assert!(js
        .sweep_launch_artifacts(&request)
        .unwrap_err()
        .contains("sweep fixture failure"));
}
