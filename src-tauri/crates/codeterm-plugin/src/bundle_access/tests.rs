use super::*;

#[test]
fn install_allows_own_registration_reads_and_resumes_after_failure() {
    let id = "bundle-access-reentrant";
    let access = BundleAccess::for_plugin(id);
    let error = access
        .install(|| {
            read_plugin_files(id, || assert!(access.read().is_none()));
            Err::<(), _>("refused".into())
        })
        .unwrap_err();
    assert_eq!(error, "refused");
    assert!(access.read().is_some());
    assert!(access.gate.try_write().is_some());
}

#[test]
fn independent_plugins_do_not_share_the_install_barrier() {
    let first = BundleAccess::for_plugin("bundle-access-first");
    let second = BundleAccess::for_plugin("bundle-access-second");
    let _reader = first.read();
    second.install(|| Ok(())).unwrap();
}
