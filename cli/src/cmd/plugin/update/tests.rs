use super::*;

#[test]
fn absent_daemon_is_the_only_local_update_path() {
    assert!(route::<()>(Ok(None), |_| panic!("must stay local")).is_none());
    let refused = route::<()>(Err("permission denied".into()), |_| panic!("must refuse"));
    assert_eq!(refused, Some(Err("permission denied".into())));
}

#[test]
fn uncertain_update_never_becomes_a_local_replay() {
    let result = route(Ok(Some(())), |_| {
        Err("connection lost after update write".into())
    });
    assert_eq!(
        result,
        Some(Err("connection lost after update write".into()))
    );
}
