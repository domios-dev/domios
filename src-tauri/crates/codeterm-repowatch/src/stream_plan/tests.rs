use super::*;

fn delivered(streams: &[StreamSpec], path: &Path) -> bool {
    streams.iter().any(|stream| {
        path.starts_with(&stream.root)
            && !stream
                .exclusions
                .iter()
                .any(|excluded| path.starts_with(excluded))
    })
}

#[test]
fn fifteen_heavy_roots_are_excluded_without_losing_clean_coverage() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    let mut heavy = vec![
        "target".to_string(),
        "node_modules".into(),
        "cli/target".into(),
    ];
    for container in ["packages", "plugins"] {
        for number in 0..6 {
            heavy.push(format!("{container}/p{number}/node_modules"));
        }
    }
    let mut clean = vec![
        root.join("root.rs"),
        root.join("cli/src/input.rs"),
        root.join("src/input.ts"),
    ];
    for directory in &heavy {
        std::fs::create_dir_all(root.join(directory)).unwrap();
    }
    for container in ["packages", "plugins"] {
        for number in 0..6 {
            clean.push(root.join(format!("{container}/p{number}/src/input.ts")));
        }
    }
    for path in &clean {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "input").unwrap();
    }
    let directory = codeterm_util::watch_plan::directory_plan(root, &[], &[]).unwrap();
    let streams = plan(root, &directory).unwrap();
    assert_eq!(heavy.len(), 15);
    assert_eq!(streams.len(), 4);
    assert!(streams
        .iter()
        .all(|stream| stream.exclusions.len() <= MAX_EXCLUSIONS));
    for directory in heavy {
        assert!(
            !delivered(&streams, &root.join(&directory).join("churn")),
            "{directory}"
        );
        assert!(!streams
            .iter()
            .any(|stream| stream.root.starts_with(root.join(&directory))));
    }
    for path in clean {
        assert!(delivered(&streams, &path), "{}", path.display());
    }
}

#[test]
fn a_single_overfull_subtree_is_refused_and_heavy_roots_take_priority() {
    let root = Path::new("repo");
    let mut directory = DirectoryPlan {
        watches: vec![],
        excluded: (0..9)
            .map(|number| ExcludedDirectory {
                path: root.join(format!("packages/p{number}/target")),
                heavy: true,
            })
            .collect(),
    };
    assert!(plan(root, &directory).unwrap_err().contains("packages"));
    directory.excluded.truncate(3);
    directory
        .excluded
        .extend((0..12).map(|number| ExcludedDirectory {
            path: root.join(format!("ordinary{number}")),
            heavy: false,
        }));
    let streams = plan(root, &directory).unwrap();
    assert_eq!(streams[0].exclusions.len(), 8);
    for candidate in directory
        .excluded
        .iter()
        .filter(|candidate| candidate.heavy)
    {
        assert!(streams[0].exclusions.contains(&candidate.path));
    }
}

#[derive(Default)]
struct Port {
    calls: Vec<&'static str>,
    fail_exclusion: bool,
}

impl StreamPort for Port {
    type Stream = ();
    fn create(&mut self, _: &Path) -> Result<(), String> {
        self.calls.push("create");
        Ok(())
    }
    fn exclude(&mut self, _: &mut (), _: &[PathBuf]) -> Result<(), String> {
        self.calls.push("exclude");
        if self.fail_exclusion {
            Err("refused".into())
        } else {
            Ok(())
        }
    }
    fn start(&mut self, _: &mut ()) -> Result<(), String> {
        self.calls.push("start");
        Ok(())
    }
}

#[test]
fn exclusions_are_installed_before_activation_and_failure_never_starts() {
    let mut port = Port::default();
    let mut spec = StreamSpec {
        root: PathBuf::from("repo"),
        exclusions: vec![PathBuf::from("repo/target")],
    };
    start(&mut port, &spec).unwrap();
    assert_eq!(port.calls, ["create", "exclude", "start"]);
    port.calls.clear();
    port.fail_exclusion = true;
    assert!(start(&mut port, &spec).is_err());
    assert_eq!(port.calls, ["create", "exclude"]);
    port.calls.clear();
    spec.exclusions = vec![PathBuf::from("ignored"); 9];
    assert!(start(&mut port, &spec).is_err());
    assert!(port.calls.is_empty());
}
