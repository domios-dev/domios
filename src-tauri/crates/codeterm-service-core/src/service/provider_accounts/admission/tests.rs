use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};

#[test]
fn removing_an_account_fences_new_launches_and_keeps_prior_reservations() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let reservation = reserve(&fixture.service, "fixture", Some("work")).unwrap();
    let state = fixture.service.ext_or_init::<State>(State::default);
    {
        let mut admissions = state.admissions.lock();
        admissions.removing.insert(
            ("fixture".into(), "work".into()),
            Removal {
                id: "removal".into(),
                working: true,
                error: None,
            },
        );
        assert_eq!(admissions.reservations.len(), 1);
    }
    assert!(!available(&fixture.service, "fixture", "work"));
    assert!(reserve(&fixture.service, "fixture", Some("work")).is_err());
    let other = reserve(&fixture.service, "fixture", Some("other")).unwrap();
    drop(reservation);
    assert_eq!(state.admissions.lock().reservations.len(), 1);
    drop(other);
    assert!(state.admissions.lock().reservations.is_empty());
}
