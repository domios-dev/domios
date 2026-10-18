use super::*;
use crate::provider_update::{available_version, ProviderUpdateHost};
use codeterm_plugin::tab_state::TabStateSnapshot;
use codeterm_plugin::{provider::*, provider_self_update::ProviderSelfUpdateCapability};
use parking_lot::Mutex;
use std::{path::PathBuf, sync::mpsc, time::Duration};

struct Provider {
    id: &'static str,
    latest: &'static str,
}
impl AgentProvider for Provider {
    fn id(&self) -> &str {
        self.id
    }
    fn display_name(&self) -> &str {
        self.id
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
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
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn supports_update_check(&self) -> bool {
        true
    }
    fn provider_self_update_capability(&self) -> Option<ProviderSelfUpdateCapability> {
        Some(ProviderSelfUpdateCapability {
            npm_package: "@fixture/agent".into(),
            standalone_directory: "standalone".into(),
            standalone_args: vec!["update".into()],
        })
    }
    fn check_for_update(&self) -> Option<ProviderUpdateAvailability> {
        Some(ProviderUpdateAvailability {
            current_version: "0.162.1".into(),
            latest_version: self.latest.into(),
        })
    }
    fn update_command(&self, _: &str) -> Option<String> {
        panic!("read-only discovery must not ask for an installer")
    }
}

#[derive(Clone)]
struct Host {
    current: Arc<Mutex<Arc<dyn AgentProvider>>>,
    published: mpsc::Sender<String>,
}
impl ProviderUpdateHost for Host {
    fn providers(&self) -> Vec<Arc<dyn AgentProvider>> {
        vec![self.current.lock().clone()]
    }
    fn provider(&self, id: &str) -> Option<Arc<dyn AgentProvider>> {
        let provider = self.current.lock().clone();
        (provider.id() == id).then_some(provider)
    }
    fn auto_update_enabled(&self) -> bool {
        false
    }
    fn matches_own_session(&self, _: &TabStateSnapshot) -> bool {
        false
    }
    fn provider_busy(&self, _: &str) -> bool {
        panic!("discovery does not wait on turns")
    }
    fn completed(&self, _: &str, _: &str) {
        panic!("discovery cannot install")
    }
    fn availability_changed(&self, provider: &str) {
        self.published.send(provider.to_owned()).unwrap();
    }
}

#[test]
fn startup_publishes_before_restore_gate_with_auto_install_disabled() {
    let (published, receipt) = mpsc::channel();
    let provider: Arc<dyn AgentProvider> = Arc::new(Provider {
        id: "startup_discovery_fixture",
        latest: "0.163.0",
    });
    let host = Host {
        current: Arc::new(Mutex::new(provider)),
        published,
    };
    let unopened = crate::provider_update::boot::RestoreGate::new();
    crate::provider_update::schedule_checks(Arc::new(host), false, &unopened);
    assert_eq!(
        receipt.recv_timeout(Duration::from_secs(3)).unwrap(),
        "startup_discovery_fixture"
    );
    let available = available_version("startup_discovery_fixture").unwrap();
    assert_eq!(available.current_version, "0.162.1");
    assert_eq!(available.latest_version, "0.163.0");
}

#[test]
fn reload_refreshes_cache_and_superseded_discovery_cannot_overwrite_it() {
    let (published, receipt) = mpsc::channel();
    let old: Arc<dyn AgentProvider> = Arc::new(Provider {
        id: "reload_discovery_fixture",
        latest: "0.163.0",
    });
    let new: Arc<dyn AgentProvider> = Arc::new(Provider {
        id: "reload_discovery_fixture",
        latest: "0.164.0",
    });
    let host = Host {
        current: Arc::new(Mutex::new(new)),
        published,
    };
    refresh_available(&host, "reload_discovery_fixture");
    assert_eq!(
        receipt.recv_timeout(Duration::from_secs(3)).unwrap(),
        "reload_discovery_fixture"
    );
    discover(&host, old);
    assert_eq!(
        available_version("reload_discovery_fixture")
            .unwrap()
            .latest_version,
        "0.164.0"
    );
    assert!(matches!(receipt.try_recv(), Err(mpsc::TryRecvError::Empty)));
}
