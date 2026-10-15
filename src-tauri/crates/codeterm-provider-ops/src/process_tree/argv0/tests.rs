use super::*;

#[test]
fn cmdline_extracts_only_the_argv0_basename() {
    assert_eq!(
        names_from_cmdline(b"/opt/bin/agent-cli\0--model\0other\0"),
        Some(ProcessNames {
            argv0_basename: "agent-cli".into(),
            script_basename: None
        })
    );
    assert_eq!(
        names_from_cmdline(b"agent-cli\0"),
        Some(ProcessNames {
            argv0_basename: "agent-cli".into(),
            script_basename: None
        })
    );
    for bytes in [
        b"".as_slice(),
        b"\0agent-cli\0",
        b"unterminated",
        b"/\0",
        b"\xff\0",
    ] {
        assert_eq!(names_from_cmdline(bytes), None);
    }
}

#[test]
fn procargs_skips_the_executable_path_and_padding() {
    let mut bytes = 2_i32.to_ne_bytes().to_vec();
    bytes
        .extend_from_slice(b"/opt/bin/native-helper\0\0\0/opt/bin/agent-cli\0--model\0KEY=value\0");
    assert_eq!(
        names_from_procargs(&bytes),
        Some(ProcessNames {
            argv0_basename: "agent-cli".into(),
            script_basename: None
        })
    );
}

#[test]
fn procargs_rejects_missing_or_invalid_argv0() {
    for suffix in [
        b"".as_slice(),
        b"/opt/helper",
        b"/opt/helper\0\0",
        b"/opt/helper\0unterminated",
        b"/opt/helper\0\xff\0",
    ] {
        let mut bytes = 1_i32.to_ne_bytes().to_vec();
        bytes.extend_from_slice(suffix);
        assert_eq!(names_from_procargs(&bytes), None);
    }
    assert_eq!(names_from_procargs(&[1, 0, 0]), None);
    let mut bytes = 0_i32.to_ne_bytes().to_vec();
    bytes.extend_from_slice(b"/opt/helper\0agent-cli\0");
    assert_eq!(names_from_procargs(&bytes), None);
}

#[test]
fn python_script_arguments_follow_interpreter_flags() {
    for bytes in [
        b"/venv/bin/python\0/venv/bin/aider\0--model\0model\0".as_slice(),
        b"python3.12\0-u\0-B\0-W\0ignore\0-X\0utf8\0/venv/bin/aider\0",
        b"Python\0-Iu\0-Wignore\0-Xutf8\0--\0/venv/bin/aider\0",
    ] {
        let names = names_from_cmdline(bytes).unwrap();
        assert_eq!(names.script_basename.as_deref(), Some("aider"));
    }
}

#[test]
fn python_code_modules_and_unrelated_arguments_are_not_scripts() {
    for bytes in [
        b"python\0-c\0aider\0".as_slice(),
        b"python\0-m\0aider\0",
        b"python\0-uc\0aider\0",
        b"python\0-W\0aider\0",
        b"python\0-X\0aider\0",
        b"python\0-\0aider\0",
        b"python\0--help\0aider\0",
        b"python-helper\0aider\0",
        b"zsh\0aider\0",
        b"python\0aider",
    ] {
        assert_eq!(names_from_cmdline(bytes).unwrap().script_basename, None);
    }
}

#[test]
fn procargs_extracts_the_script_without_reading_environment() {
    let mut bytes = 3_i32.to_ne_bytes().to_vec();
    bytes.extend_from_slice(b"/real/Python\0\0/venv/python3.12\0-u\0/venv/bin/aider\0KEY=value\0");
    let names = names_from_procargs(&bytes).unwrap();
    assert_eq!(names.argv0_basename, "python3.12");
    assert_eq!(names.script_basename.as_deref(), Some("aider"));

    let mut bytes = 1_i32.to_ne_bytes().to_vec();
    bytes.extend_from_slice(b"/real/Python\0\0python\0aider\0");
    assert_eq!(names_from_procargs(&bytes).unwrap().script_basename, None);
}
