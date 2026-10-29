use super::*;
use std::time::Duration;

struct Allow;
impl RequestObserver for Allow {
    fn before_request(&self) -> Result<(), DecisionError> {
        Ok(())
    }
}

#[test]
fn observation_only_physical_scope_survives_nested_allow_unwind_and_worker_projection() {
    assert!(effects_allowed());
    let deadline = Instant::now() + Duration::from_secs(5);
    {
        let _deny = RequestScope::observation_only(deadline);
        assert!(!effects_allowed());
        assert!(before_request().is_err());
        let _ = std::panic::catch_unwind(|| {
            let _allow = RequestScope::enter(Arc::new(Allow));
            assert!(!effects_allowed());
            assert!(before_request().is_err());
            panic!("fixture unwind");
        });
        assert!(!effects_allowed());
        let observer = current().unwrap();
        std::thread::spawn(move || {
            let _scope = RequestScope::enter(observer);
            assert!(!effects_allowed());
            assert!(before_request().is_err());
            assert_eq!(super::deadline(), Some(deadline));
        })
        .join()
        .unwrap();
    }
    assert!(effects_allowed());
    assert!(before_request().is_ok());
    let _allow = RequestScope::enter(Arc::new(Allow));
    assert!(effects_allowed());
}
