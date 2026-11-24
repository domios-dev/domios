use super::*;

fn peer(origin: AuthenticatedOrigin, access: AccessLevel) -> Caller {
    Caller::AccountPeer { origin, access }
}

#[test]
fn general_main_can_store_on_a_full_access_account_peer() {
    use Operation::*;

    assert_eq!(
        authorize(
            Caller::AdmittedMain {
                access: AccessLevel::Full
            },
            Store
        ),
        Ok(())
    );
    assert_eq!(
        authorize(
            Caller::AdmittedMain {
                access: AccessLevel::Full
            },
            Commit
        ),
        Err(OWNER_REQUIRED)
    );
    assert_eq!(
        authorize(
            Caller::AdmittedMain {
                access: AccessLevel::Full
            },
            Inspect
        ),
        Err(MAIN_OR_OWNER_REQUIRED)
    );
    assert_eq!(
        authorize(
            Caller::AdmittedMain {
                access: AccessLevel::ReadOnly
            },
            Store
        ),
        Err(FULL_ACCESS_REQUIRED)
    );
}

#[test]
fn owner_can_commit_and_other_callers_have_no_proposal_authority() {
    use AuthenticatedOrigin::*;
    use Operation::*;

    assert_eq!(authorize(Caller::LocalOwner, Commit), Ok(()));
    assert_eq!(authorize(peer(Owner, AccessLevel::Full), Commit), Ok(()));
    assert_eq!(authorize(Caller::LocalMain, Commit), Err(OWNER_REQUIRED));
    assert_eq!(
        authorize(peer(Agent, AccessLevel::Full), Store),
        Err(MAIN_OR_OWNER_REQUIRED)
    );
    assert_eq!(
        authorize(peer(Mesh, AccessLevel::Full), Store),
        Err(MAIN_OR_OWNER_REQUIRED)
    );
    assert_eq!(authorize(Caller::Other, Store), Err(MAIN_OR_OWNER_REQUIRED));
}
