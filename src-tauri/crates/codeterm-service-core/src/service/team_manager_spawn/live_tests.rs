//! The socket `agent.spawn` door (`spawn_agent_sync`) end to end: admission, parenting, and the
//! authority a custom manager never gains.

use codeterm_team_package::{RoleKind, RoleStart, StartMode};
use codeterm_types::agent::AgentRole;
use codeterm_types::team::role_registration::{PinnedPackage, RoleRegistration};
use codeterm_types::team::staffing::{TEAM_ROLE_ALREADY_LIVE, TEAM_ROLE_REQUIRES_ORCHESTRATOR};

use crate::multiplexer::state::MultiplexerState;
use crate::service::orchestrator::SpawnAgentParams;
use crate::service::team_roles::TeamRoleStore;
use crate::service::test_fixture::ServiceFixture;

const PROVIDER: &str = "custom-manager-fixture";
const NAME: &str = "security_lead";

struct Team {
    fixture: ServiceFixture,
    workspace: String,
    orchestrator: String,
    team_id: String,
}

fn team() -> Team {
    let mut mux = MultiplexerState::empty();
    let workspace = mux.create_workspace("custom manager".into(), None).id;
    let orchestrator = mux.create_tab(&workspace).unwrap();
    mux.set_orchestrator_role(&orchestrator).unwrap();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&orchestrator)
        .unwrap()
        .team_id;
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new(PROVIDER)
            .with_launch_binary(crate::agent::test_provider::test_shell_binary()),
    ));
    TeamRoleStore::global()
        .register(RoleRegistration {
            team_id: team_id.clone(),
            kind: RoleKind::Manager,
            name: NAME.into(),
            label: "Warden".into(),
            description: None,
            start: RoleStart::Mode(StartMode::OnDemand),
            package: PinnedPackage {
                name: "dev.example/fixture".into(),
                version: "1.0.0".into(),
            },
            instructions: "fixture-prose".into(),
            model: None,
            reasoning: None,
            placement_peer: None,
            consent_decision_id: None,
            files: Vec::new(),
            skills: Vec::new(),
            registered_at: 7,
        })
        .unwrap();
    Team {
        fixture,
        workspace,
        orchestrator,
        team_id,
    }
}

impl Team {
    fn spawn(
        &self,
        role: &str,
        role_profile: Option<&str>,
        spawned_by: &str,
        parent: Option<&str>,
    ) -> Result<String, String> {
        self.fixture
            .service
            .spawn_agent_sync(SpawnAgentParams {
                agent: Some(PROVIDER.into()),
                role: Some(role.into()),
                role_profile: role_profile.map(str::to_string),
                spawned_by_tab_id: Some(spawned_by.into()),
                parent_tab_id: parent.map(str::to_string),
                workspace_id: Some(self.workspace.clone()),
                ..Default::default()
            })
            .map(|result| result.tab_id)
    }

    fn tab_count(&self) -> usize {
        self.fixture.service.mux.read().all_tabs().count()
    }

    fn close(&self, tab_id: &str) {
        self.fixture.service.close_tab(tab_id).unwrap();
    }
}

#[test]
fn the_orchestrator_starts_one_parented_custom_manager_and_a_second_is_refused() {
    let team = team();
    let manager = team
        .spawn("Manager", Some(NAME), &team.orchestrator, None)
        .expect("the Orchestrator starts its registered manager without --parent-tab");
    {
        let mux = team.fixture.service.mux.read();
        let tab = mux.get_tab(&manager).unwrap();
        assert_eq!(tab.role, Some(AgentRole::Manager));
        assert_eq!(tab.role_kind_id(), Some(NAME));
        assert_eq!(
            tab.parent_tab_id.as_deref(),
            Some(team.orchestrator.as_str())
        );
        assert_eq!(tab.team_id.as_deref(), Some(team.team_id.as_str()));
        assert!(!tab.can_spawn);
    }

    let before = team.tab_count();
    let second = team
        .spawn(
            "Manager",
            Some(NAME),
            &team.orchestrator,
            Some(&team.orchestrator),
        )
        .unwrap_err();
    assert!(second.starts_with(TEAM_ROLE_ALREADY_LIVE), "{second}");
    assert_eq!(team.tab_count(), before);
    team.close(&manager);
}

#[test]
fn a_custom_manager_starts_nothing() {
    let team = team();
    let manager = team
        .spawn("Manager", Some(NAME), &team.orchestrator, None)
        .unwrap();
    let before = team.tab_count();

    let peer = team
        .spawn("Manager", Some(NAME), &manager, None)
        .unwrap_err();
    assert!(peer.starts_with(TEAM_ROLE_REQUIRES_ORCHESTRATOR), "{peer}");
    assert!(team
        .spawn("Worker", None, &manager, Some(&manager))
        .is_err());
    assert_eq!(team.tab_count(), before);
    team.close(&manager);
}

#[test]
fn a_non_orchestrator_cannot_start_a_custom_manager() {
    let team = team();
    let worker = team
        .spawn("Worker", None, &team.orchestrator, Some(&team.orchestrator))
        .unwrap();
    let before = team.tab_count();
    let refused = team
        .spawn("Manager", Some(NAME), &worker, Some(&team.orchestrator))
        .unwrap_err();
    assert!(
        refused.starts_with(TEAM_ROLE_REQUIRES_ORCHESTRATOR),
        "{refused}"
    );
    assert_eq!(team.tab_count(), before);
    team.close(&worker);
}
