use super::*;

fn peer(origin: AuthenticatedOrigin, main: bool, access: AccessLevel) -> Caller {
    Caller::AccountPeer {
        origin,
        main,
        access,
    }
}

#[test]
fn general_main_can_store_on_a_full_access_account_peer() {
    use AuthenticatedOrigin::Agent;
    use Operation::*;

    assert_eq!(
        authorize(peer(Agent, true, AccessLevel::Full), Store),
        Ok(())
    );
    assert_eq!(
        authorize(peer(Agent, true, AccessLevel::Full), Commit),
        Err(OWNER_REQUIRED)
    );
    assert_eq!(
        authorize(peer(Agent, true, AccessLevel::Full), Inspect),
        Err(MAIN_OR_OWNER_REQUIRED)
    );
    assert_eq!(
        authorize(peer(Agent, true, AccessLevel::ReadOnly), Store),
        Err(FULL_ACCESS_REQUIRED)
    );
}

#[test]
fn owner_can_commit_and_other_callers_have_no_proposal_authority() {
    use AuthenticatedOrigin::*;
    use Operation::*;

    assert_eq!(authorize(Caller::LocalOwner, Commit), Ok(()));
    assert_eq!(
        authorize(peer(Owner, false, AccessLevel::Full), Commit),
        Ok(())
    );
    assert_eq!(authorize(Caller::LocalMain, Commit), Err(OWNER_REQUIRED));
    assert_eq!(
        authorize(peer(Agent, false, AccessLevel::Full), Store),
        Err(MAIN_OR_OWNER_REQUIRED)
    );
    assert_eq!(
        authorize(peer(Mesh, false, AccessLevel::Full), Store),
        Err(MAIN_OR_OWNER_REQUIRED)
    );
    assert_eq!(authorize(Caller::Other, Store), Err(MAIN_OR_OWNER_REQUIRED));
}
