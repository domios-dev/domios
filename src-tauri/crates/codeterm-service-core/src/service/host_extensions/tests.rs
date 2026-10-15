use super::*;

#[test]
fn registration_is_typed_set_once_and_preserves_the_original_arc() {
    let extensions = HostExtensions::default();
    assert!(extensions.get::<u32>().is_none());
    let first = Arc::new(7_u32);
    assert!(extensions.register(first.clone()).is_ok());
    let duplicate = Arc::new(9_u32);
    let refused = extensions.register(duplicate.clone()).unwrap_err();
    assert!(Arc::ptr_eq(&refused, &duplicate));
    assert!(Arc::ptr_eq(&extensions.get::<u32>().unwrap(), &first));
    assert!(extensions.get::<String>().is_none());
}

#[test]
fn independent_contexts_keep_separate_host_services() {
    let first = HostExtensions::default();
    let second = HostExtensions::default();
    assert!(first.register(Arc::new(7_u32)).is_ok());
    assert!(second.register(Arc::new(9_u32)).is_ok());
    assert_eq!(*first.get::<u32>().unwrap(), 7);
    assert_eq!(*second.get::<u32>().unwrap(), 9);
}

#[test]
fn concurrent_registration_has_one_winner() {
    let extensions = HostExtensions::default();
    let barrier = std::sync::Barrier::new(2);
    let winners = std::thread::scope(|scope| {
        let handles: Vec<_> = [7_u32, 9_u32]
            .into_iter()
            .map(|value| {
                let extensions = &extensions;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    extensions.register(Arc::new(value)).is_ok()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|won| *won)
            .count()
    });
    assert_eq!(winners, 1);
    assert!([7, 9].contains(&*extensions.get::<u32>().unwrap()));
}
