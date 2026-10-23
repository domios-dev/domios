use super::*;
use std::sync::Arc;

#[test]
fn blueprint_v2_runtime_bundled_probe_recipes_execute() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../resources/blueprints");
    let mut count = 0;
    for file in std::fs::read_dir(root).unwrap() {
        let path = file.unwrap().path();
        if path.extension().and_then(|suffix| suffix.to_str()) != Some("yaml") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        let document =
            codeterm_blueprint_types::blueprint_document::parse(path.to_str().unwrap(), &source)
                .unwrap()
                .document;
        if document.steps.first().map(|node| node.kind()) != Some("probe") {
            continue;
        }
        count += 1;
        let directory = tempfile::tempdir().unwrap();
        let environment = fixture_tools(directory.path());
        let bindings = document
            .inputs
            .iter()
            .map(|(name, _)| {
                let value = if name == "name" {
                    format!("probe-recipe-{count}")
                } else if name.contains("probe") {
                    if cfg!(windows) {
                        "gh.cmd blueprint-fixture"
                    } else {
                        "gh blueprint-fixture"
                    }
                    .into()
                } else if name.starts_with("failed") {
                    r"(?m)^failed\r?$".into()
                } else {
                    r"(?m)^passed\r?$".into()
                };
                format!("{name}={value}")
            })
            .collect::<Vec<_>>();
        let bound = document.bind(&bindings).unwrap();
        let (fixture, storage, handle, lease) = fixture(bound, directory.path());
        let (_, diagnostic) = crate::probe_env::with_command_environment(environment, || {
            crate::scheduler::tick(&handle, &Arc::new(fixture.service.clone()), &lease);
        });
        assert_eq!(
            std::fs::read_to_string(directory.path().join("gh-invoked"))
                .unwrap_or_else(|error| panic!(
                    "{}: fixture marker: {error}; {diagnostic:?}",
                    path.display()
                ))
                .trim(),
            "blueprint-fixture",
            "{}; {diagnostic:?}",
            path.display()
        );
        let current = handle.store.lock().get(&lease.job.id).unwrap().clone();
        let expected = if lease.job.buckets.iter().any(|bucket| bucket == "alldone") {
            "alldone"
        } else {
            "passed"
        };
        assert_eq!(
            current.machine.last_bucket.as_deref(),
            Some(expected),
            "{}: verdict_reason={:?}; {diagnostic:?}",
            path.display(),
            current.last_reason
        );
        assert!(current
            .last_reason
            .as_deref()
            .is_some_and(|reason| !reason.starts_with("probe_")));
        assert_eq!(handle.probe_owner.snapshot().unconfirmed, 0);
        drop(handle);
        drop(fixture);
        close(storage);
    }
    assert_eq!(count, 4);
}

fn fixture_tools(root: &Path) -> crate::probe::environment::Environment {
    let bin = root.join("bin");
    std::fs::create_dir(&bin).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let tool = bin.join("gh");
        std::fs::write(&tool, "#!/bin/sh\n[ \"$#\" -eq 1 ] && [ \"$1\" = blueprint-fixture ] || exit 64\nprintf '%s\\n' \"$1\" > gh-invoked\nprintf '%s\\n' passed\n").unwrap();
        std::fs::set_permissions(tool, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[cfg(windows)]
    std::fs::write(bin.join("gh.cmd"), "@echo off\r\nif not \"%~1\"==\"blueprint-fixture\" exit /b 64\r\necho %~1>gh-invoked\r\necho passed\r\n").unwrap();
    let inherited = std::iter::once(("PATH".into(), bin.into_os_string()))
        .chain(std::env::var_os("SystemRoot").map(|root| ("SystemRoot".into(), root)));
    crate::probe::environment::Environment::for_command(inherited, None, &[])
}
