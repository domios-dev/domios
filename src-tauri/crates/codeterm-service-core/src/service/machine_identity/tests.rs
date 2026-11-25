use super::*;
use crate::{
    multiplexer::state::MultiplexerState,
    ports::daemon_question::{DaemonQuestionHandler, DaemonQuestions},
    service::test_fixture::ServiceFixture,
};
use mesh::machine_identity::MachineKind;
use std::sync::Arc;

struct OtherQuestions;

impl DaemonQuestionHandler for OtherQuestions {
    fn answer(&self, _: &str, _: &str) -> Option<Result<Value, String>> {
        None
    }
}

#[test]
fn requested_machine_identity_requires_the_owner_and_cannot_overwrite_a_new_choice() {
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let service = &fixture.service;
    assert!(service
        .register_host_extension(Arc::new(DaemonQuestions::new(Arc::new(OtherQuestions))))
        .is_ok());
    let mesh = service.try_ext::<MeshStateHandle>().unwrap();
    let original = mesh.lock().machine_identity();
    let choice = MachineIdentity {
        kind: MachineKind::Laptop,
    };
    let request = service.request_machine_identity(choice).unwrap();
    let id = request["notification_id"].as_str().unwrap();
    assert_eq!(request["state"], "awaiting_owner");
    assert!(service.answer_notification(id, "Apply").is_err());
    assert_eq!(mesh.lock().machine_identity(), original);
    assert_eq!(
        service.answer_notification_as_owner(id, "Apply").unwrap()["state"],
        "saved"
    );
    assert_eq!(mesh.lock().machine_identity(), choice);

    let old_request = service
        .request_machine_identity(MachineIdentity {
            kind: MachineKind::Server,
        })
        .unwrap();
    let newer = MachineIdentity {
        kind: MachineKind::Desktop,
    };
    service.update_machine_identity(newer, None).unwrap();
    assert!(service
        .answer_notification_as_owner(old_request["notification_id"].as_str().unwrap(), "Apply")
        .is_err());
    assert_eq!(mesh.lock().machine_identity(), newer);
}
