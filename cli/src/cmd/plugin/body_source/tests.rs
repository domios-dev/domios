use super::*;

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

#[test]
fn conflicting_sources_fail_before_reading() {
    for first in [
        vec!["--from-pad", "body.md"],
        vec!["--from-file", "-"],
        vec!["--stdin"],
    ] {
        for second in [
            vec!["--from-pad", "other.md"],
            vec!["--from-file", "-"],
            vec!["--stdin"],
        ] {
            let mut args = first.clone();
            args.extend(second);
            let error = resolve_with(
                strings(&args),
                |_| panic!("read pad"),
                |_, _| panic!("read file"),
            )
            .unwrap_err();
            assert!(error.contains("only one body source"));
        }
    }
}

#[test]
fn missing_source_operands_fail_before_reading() {
    for args in [
        vec!["--from-pad"],
        vec!["--from-file"],
        vec!["--from-pad", "--"],
        vec!["--from-file", "--stdin"],
        vec!["--from-pad", ""],
    ] {
        assert!(resolve_with(
            strings(&args),
            |_| panic!("read pad"),
            |_, _| panic!("read file")
        )
        .unwrap_err()
        .contains("requires a source path"));
    }
}

#[test]
fn inline_and_recipient_arguments_are_unchanged() {
    for args in [
        vec!["--message", "inline text"],
        vec!["--", "recipient", "--from-pad", "literal"],
        vec![],
    ] {
        let expected = strings(&args);
        assert_eq!(
            resolve_with(
                expected.clone(),
                |_| panic!("read pad"),
                |_, _| panic!("read file")
            )
            .unwrap(),
            (expected, None)
        );
    }
}

#[test]
fn stdin_flags_resolve_to_dash_without_touching_args() {
    for source in [
        vec!["--stdin"],
        vec!["--from-file", "-"],
        vec!["--from-pad", "-"],
    ] {
        let mut args = vec!["--chat", "target"];
        args.extend(source);
        args.extend(["--", "--from-file", "recipient"]);
        let expected = "--message\r\nλ😀\n";
        let read_pad = |path: &str| {
            assert_eq!(path, "-");
            Ok(expected.to_string())
        };
        let read_file = |path: &str, _: &str| {
            assert_eq!(path, "-");
            Ok(expected.to_string())
        };
        assert_eq!(
            resolve_with(strings(&args), read_pad, read_file).unwrap(),
            (
                strings(&["--chat", "target", "--", "--from-file", "recipient"]),
                Some(expected.to_string())
            )
        );
    }
}

#[test]
fn pad_read_errors_stop_preparation_and_paths_stay_confined() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("pad");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(temp.path().join("outside.md"), "foreign").unwrap();
    for path in ["missing.md", "../outside.md"] {
        let result = resolve_with(
            strings(&["--from-pad", path]),
            |relative| payload_source::read_from_pad_root(&root, relative),
            |_, _| panic!("read file"),
        );
        assert!(result.is_err());
    }
}

#[test]
fn file_and_pad_bodies_preserve_utf8_bytes() {
    let temp = tempfile::tempdir().unwrap();
    for expected in ["--message", "--", "", "quotes ` $value\r\nλ😀\n"] {
        let file = temp.path().join("body.md");
        std::fs::write(&file, expected).unwrap();
        for source in ["--from-file", "--from-pad"] {
            let path = if source == "--from-pad" {
                "body.md"
            } else {
                file.to_str().unwrap()
            };
            let (args, body) = resolve_with(
                strings(&[source, path]),
                |relative| payload_source::read_from_pad_root(temp.path(), relative),
                payload_source::read_file_or_stdin,
            )
            .unwrap();
            assert!(args.is_empty());
            assert_eq!(body.unwrap().as_bytes(), expected.as_bytes());
        }
    }
}

#[cfg(unix)]
#[test]
fn pad_symlink_escape_fails_preparation() {
    let temp = tempfile::tempdir().unwrap();
    let pad = temp.path().join("pad");
    std::fs::create_dir(&pad).unwrap();
    let outside = temp.path().join("outside.md");
    std::fs::write(&outside, "foreign").unwrap();
    std::os::unix::fs::symlink(outside, pad.join("escape.md")).unwrap();
    let error = resolve_with(
        strings(&["--from-pad", "escape.md"]),
        |path| payload_source::read_from_pad_root(&pad, path),
        |_, _| panic!("read file"),
    )
    .unwrap_err();
    assert!(error.contains("escapes"));
}
