use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use crate::session::store::StorageDeskAdapter;
use crate::startup::StorageAuthority;
use crate::storage::Storage;
use std::path::Path;
use std::time::Duration;
use tauri::Manager;

const DEADLINE: Duration = Duration::from_secs(30);

pub struct ServiceInstance {
    pub fixture: ServiceFixture,
    pub storage: Storage,
    authority: StorageAuthority,
}

impl ServiceInstance {
    pub fn open_with_mux(root: &Path, mux: MultiplexerState) -> Self {
        let authority = crate::startup::open_instance_storage_at(root);
        let storage = authority
            .require_ready()
            .expect("startup opens ready Storage")
            .clone();
        let fixture = ServiceFixture::new_at(mux, root);
        assert!(fixture._app.manage(authority.clone()));
        let app = fixture._app.handle().clone();
        crate::service::session_persist::install_storage_desk_adapter(
            &app,
            StorageDeskAdapter::from_storage(storage.clone()),
        )
        .expect("startup installs the Storage desk adapter");
        crate::service::session_persist::start_worker(&app)
            .expect("startup starts the desk persistence worker");
        let is_empty = StorageDeskAdapter::from_storage(storage.clone())
            .snapshot()
            .expect("read startup desk")
            .workspaces
            .is_empty();
        if is_empty {
            crate::service::session_persist::write_session_snapshot_with_intent(
                &app,
                &fixture.service.mux,
                crate::session::store::SaveIntent::AuthorizedReduction,
            )
            .expect("startup boot resync commits the initialized desk");
        }
        Self {
            fixture,
            storage,
            authority,
        }
    }

    fn open(root: &Path) -> Self {
        let authority = crate::startup::open_instance_storage_at(root);
        let storage = authority
            .require_ready()
            .expect("startup opens ready Storage")
            .clone();
        let snapshot = StorageDeskAdapter::from_storage(storage.clone())
            .snapshot()
            .expect("startup restores the Storage desk");
        let mux = if snapshot.workspaces.is_empty() {
            MultiplexerState::new().0
        } else {
            MultiplexerState::from_session_snapshot(&snapshot)
        };
        let fixture = ServiceFixture::new_at(mux, root);
        assert!(fixture._app.manage(authority.clone()));
        let app = fixture._app.handle().clone();
        crate::service::session_persist::install_storage_desk_adapter(
            &app,
            StorageDeskAdapter::from_storage(storage.clone()),
        )
        .expect("startup installs the Storage desk adapter");
        crate::service::session_persist::start_worker(&app)
            .expect("startup starts the desk persistence worker");
        let is_empty = snapshot.workspaces.is_empty();
        if is_empty {
            crate::service::session_persist::write_session_snapshot_with_intent(
                &app,
                &fixture.service.mux,
                crate::session::store::SaveIntent::AuthorizedReduction,
            )
            .expect("startup boot resync commits its initialized desk");
        }
        Self {
            fixture,
            storage,
            authority,
        }
    }

    pub fn restart(self, root: &Path) -> Self {
        self.fixture.service.kpi.drain();
        let app = self.fixture._app.handle().clone();
        crate::service::session_persist::stop_worker(&app)
            .expect("the old daemon drains desk persistence before shutdown");
        drop(self.fixture);
        drop(self.authority);
        self.storage
            .shutdown()
            .wait(DEADLINE)
            .expect("the old Storage owner closes before restart");
        Self::open(root)
    }
}

pub fn started(root: &Path) -> ServiceInstance {
    ServiceInstance::open_with_mux(root, MultiplexerState::new().0)
}

pub fn close(instance: ServiceInstance) {
    instance.fixture.service.kpi.drain();
    let app = instance.fixture._app.handle().clone();
    crate::service::session_persist::stop_worker(&app)
        .expect("desk persistence worker stops cleanly");
    drop(instance.fixture);
    drop(instance.authority);
    instance
        .storage
        .shutdown()
        .wait(DEADLINE)
        .expect("Storage shuts down cleanly");
}
