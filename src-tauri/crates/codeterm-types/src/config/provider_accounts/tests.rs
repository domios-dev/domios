use super::*;

fn roster() -> ProviderAccountsConfig {
    ProviderAccountsConfig {
        profiles: ["work", "personal"]
            .map(|id| (id.into(), ProviderAccountConfig::default()))
            .into(),
        order: vec!["work".into(), IMPLICIT_ACCOUNT.into(), "personal".into()],
        workspaces: [("project".into(), "personal".into())].into(),
        ..Default::default()
    }
}

#[test]
fn implicit_account_is_preserved_without_a_new_config_namespace() {
    let config = super::super::schema::ProviderConfig::default();
    assert!(serde_json::to_value(&config)
        .unwrap()
        .get("accounts")
        .is_none());
    let accounts = ProviderAccountsConfig::default();
    assert_eq!(
        accounts.starting_account(None, None).unwrap(),
        IMPLICIT_ACCOUNT
    );
    assert_eq!(
        accounts.first_eligible(IMPLICIT_ACCOUNT, |_| true).unwrap(),
        Some(IMPLICIT_ACCOUNT)
    );
    assert_eq!(
        accounts.next_eligible(IMPLICIT_ACCOUNT, |_| true).unwrap(),
        None
    );
}

#[test]
fn spawn_workspace_and_roster_choices_have_one_precedence() {
    let accounts = roster();
    assert_eq!(accounts.starting_account(None, None).unwrap(), "work");
    assert_eq!(
        accounts.starting_account(Some("project"), None).unwrap(),
        "personal"
    );
    assert_eq!(
        accounts
            .starting_account(Some("project"), Some("default"))
            .unwrap(),
        "default"
    );
    assert!(accounts
        .starting_account(Some("project"), Some("removed"))
        .is_err());
    let restored: ProviderAccountsConfig =
        toml::from_str(&toml::to_string(&accounts).unwrap()).unwrap();
    assert_eq!(restored, accounts);
}

#[test]
fn eligibility_walks_once_in_order_and_wraps_without_reusing_current() {
    let accounts = roster();
    let mut visited = Vec::new();
    assert_eq!(
        accounts
            .next_eligible("personal", |id| {
                visited.push(id.to_owned());
                id == "default"
            })
            .unwrap(),
        Some("default")
    );
    assert_eq!(visited, ["work", "default"]);
    visited.clear();
    assert_eq!(
        accounts
            .next_eligible("default", |id| {
                visited.push(id.to_owned());
                false
            })
            .unwrap(),
        None
    );
    assert_eq!(visited, ["personal", "work"]);
    assert!(accounts.next_eligible("removed", |_| true).is_err());
}

#[test]
fn removal_clears_workspace_choices_and_preserves_the_usual_home() {
    let mut accounts = roster();
    assert!(accounts.remove(IMPLICIT_ACCOUNT).is_err());
    accounts.remove("personal").unwrap();
    assert!(!accounts.workspaces.contains_key("project"));
    assert_eq!(accounts.order, ["work", "default"]);
    assert!(!accounts.profiles.contains_key("personal"));
    assert!(accounts.validate().is_ok());
}

#[test]
fn invalid_orders_names_and_path_like_ids_are_refused() {
    for order in [
        vec!["work", "default"],
        vec!["work", "default", "work"],
        vec!["work", "personal", "foreign"],
    ] {
        let accounts = ProviderAccountsConfig {
            order: order.into_iter().map(str::to_owned).collect(),
            ..roster()
        };
        assert!(accounts.validate().is_err());
    }
    for id in [
        "../other", "a/b", "a\\b", "", "CON", "nul", "com1", "lpt9", "c:home",
    ] {
        assert!(!valid_account_id(id), "accepted {id:?}");
    }
    let mut accounts = roster();
    accounts.profiles.get_mut("work").unwrap().name = Some("  spaced ".into());
    assert!(accounts.validate().is_err());
    accounts.profiles.get_mut("work").unwrap().name = Some("valid name".into());
    assert!(accounts.validate().is_ok());
}

#[test]
fn account_and_workspace_counts_are_bounded() {
    let mut accounts = ProviderAccountsConfig::default();
    for index in 0..MAX_PROVIDER_ACCOUNTS - 1 {
        let id = format!("account-{index}");
        accounts
            .profiles
            .insert(id.clone(), ProviderAccountConfig::default());
        accounts.order.push(id);
    }
    assert!(accounts.validate().is_ok());
    accounts
        .profiles
        .insert("overflow".into(), ProviderAccountConfig::default());
    accounts.order.push("overflow".into());
    assert!(accounts.validate().is_err());
    let mut accounts = roster();
    accounts.workspaces = (0..257)
        .map(|index| (format!("workspace-{index}"), "work".into()))
        .collect();
    assert!(accounts.validate().is_err());
}
