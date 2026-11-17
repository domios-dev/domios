use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};

#[test]
fn an_unverified_draft_is_not_a_roster_account_and_cancellation_requires_its_own_id() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let provider = format!("account-fixture-{}", uuid::Uuid::new_v4().simple());
    let usual = fixture.temp.path().join("usual-home");
    std::fs::create_dir(&usual).unwrap();
    std::fs::write(usual.join("auth.json"), "opaque-usual-auth").unwrap();
    super::super::test_support::configure(
        &fixture.service,
        fixture.temp.path().join("account-data"),
        fixture.temp.path().join("config.json"),
    )
    .unwrap();
    let manifest = PluginManifest::from_json_str(&json!({
        "id":provider,"version":"1.0.0","displayName":"Fixture",
        "capabilities":{"agentProvider":true,"binaryName":"fixture-cli","cliBinary":"fixture-cli","accounts":{
            "homeEnvironment":"ACCOUNT_ACTOR_TEST_HOME","defaultHome":usual,"secretEnvironment":["ACCOUNT_ACTOR_TEST_TOKEN"],
            "isolatedPaths":["auth.json"],"sharedPaths":[],"continuation":"unverified"
        }},
        "cliLogin":{"methods":[{"id":"browser","label":"Browser","args":["login"]}],"urlPattern":"(https://example.test/[^ ]+)","timeoutMs":1000,"codeInput":false}
    }).to_string()).unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(
            manifest,
            Arc::new(PluginVm::from_source(&provider, "module.exports={}".into()).unwrap()),
        )));
    begin(&fixture.service, &provider, None).unwrap();
    let state = fixture.service.ext_or_init::<State>(State::default);
    let entry = state.sign_ins.lock().get(&provider).cloned().unwrap();
    assert!(policy(&fixture.service, &provider).profiles.is_empty());
    let context = entry.context.as_ref().unwrap();
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&context.tab)
        .is_some_and(|tab| tab.hidden
            && tab.skip_snapshot
            && super::super::selected_account(tab) == entry.account));
    let home = state
        .root
        .join("provider-accounts")
        .join(&provider)
        .join(&entry.account);
    std::fs::write(home.join("auth.json"), "opaque-draft-auth").unwrap();
    assert!(cancel(&fixture.service, &provider, None, Some("foreign-attempt")).is_err());
    assert!(home.exists());
    cancel(&fixture.service, &provider, None, Some(&entry.generation)).unwrap();
    assert!(!home.exists());
    assert!(fixture.service.mux.read().get_tab(&context.tab).is_none());
    assert!(!state.sign_ins.lock().contains_key(&provider));
    assert!(policy(&fixture.service, &provider).profiles.is_empty());
    assert_eq!(
        std::fs::read_to_string(usual.join("auth.json")).unwrap(),
        "opaque-usual-auth"
    );
}
