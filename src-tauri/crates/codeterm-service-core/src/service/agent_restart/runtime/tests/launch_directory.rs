use super::*;

fn launched_cwds(fixture: &RuntimeFixture) -> (String, String, Value) {
    let tab_cwd = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .unwrap()
        .cwd
        .clone()
        .unwrap()
        .to_string();
    let spawned = agent_runtime::process::test_ports::spawned_cwd(&fixture.tab).unwrap();
    let thread = fixture
        .requests()
        .into_iter()
        .find(|request| request["method"] == "resumeThread")
        .expect("the launch loads the tab's thread");
    (tab_cwd, spawned, thread["params"]["cwd"].clone())
}

#[test]
fn a_host_working_directory_profile_starts_outside_the_project_while_threads_keep_the_tab_cwd() {
    let source = SOURCE.replace("params:op}", "params:{...op,cwd:ctx.cwd}}");
    let hosted = RuntimeFixture::prepared_with_launch_profiles(
        &source,
        true,
        Some(json!({"hosted":{"binary":"node","args":[],"hostWorkingDirectory":true}})),
    );
    agent_runtime::transport::launch(
        &hosted.service,
        PROVIDER,
        &hosted.tab,
        json!({"launchProfile":"hosted","systemPrompt":PROMPT,"model":"supplied-model"}),
    )
    .unwrap();
    let (tab_cwd, spawned, thread_cwd) = launched_cwds(&hosted);
    let owned = agent_runtime::launch_profile_install::profile_root(PROVIDER)
        .unwrap()
        .join("hosted");
    assert!(
        std::path::Path::new(&spawned).starts_with(&owned),
        "{spawned}"
    );
    assert!(std::path::Path::new(&spawned).is_dir());
    assert_ne!(spawned, tab_cwd);
    assert_eq!(thread_cwd, json!(tab_cwd));
    drop(hosted);

    let plain = RuntimeFixture::with_source(&source);
    let (tab_cwd, spawned, thread_cwd) = launched_cwds(&plain);
    assert_eq!(spawned, tab_cwd);
    assert_eq!(thread_cwd, json!(tab_cwd));
}
