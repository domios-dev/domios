#[derive(Debug, PartialEq, Eq)]
pub(super) struct ProcessNames {
    pub(super) argv0_basename: String,
    pub(super) script_basename: Option<String>,
}

fn basename(bytes: &[u8]) -> Option<String> {
    let name = std::str::from_utf8(bytes).ok()?.rsplit('/').next()?;
    (!name.is_empty()).then(|| name.to_owned())
}

pub(super) fn names_from_cmdline(bytes: &[u8]) -> Option<ProcessNames> {
    let end = bytes.iter().position(|byte| *byte == 0)?;
    let argv0_basename = basename(bytes.get(..end)?)?;
    let script_basename = python_script_basename(&argv0_basename, bytes.get(end + 1..)?);
    Some(ProcessNames {
        argv0_basename,
        script_basename,
    })
}

fn python_script_basename(interpreter: &str, bytes: &[u8]) -> Option<String> {
    let interpreter = interpreter.to_ascii_lowercase();
    let version = interpreter.strip_prefix("python")?;
    if !version.is_empty()
        && !version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    let mut args = bytes.split_inclusive(|byte| *byte == 0);
    while let Some(arg) = args.next() {
        let arg = std::str::from_utf8(arg.strip_suffix(&[0])?).ok()?;
        if arg == "--" {
            return basename(args.next()?.strip_suffix(&[0])?);
        }
        if !arg.starts_with('-') {
            return basename(arg.as_bytes());
        }
        if arg == "-W" || arg == "-X" {
            args.next()?.strip_suffix(&[0])?;
        } else if arg.starts_with("-W") || arg.starts_with("-X") {
            continue;
        } else if arg.len() < 2
            || !arg[1..]
                .bytes()
                .all(|byte| b"bBdEiIOPqRsSuvx".contains(&byte))
        {
            return None;
        }
    }
    None
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn names_from_procargs(bytes: &[u8]) -> Option<ProcessNames> {
    let argc = i32::from_ne_bytes(bytes.get(..4)?.try_into().ok()?);
    if argc <= 0 {
        return None;
    }
    let bytes = bytes.get(4..)?;
    let executable_end = bytes.iter().position(|byte| *byte == 0)?;
    let args = bytes.get(executable_end + 1..)?;
    let argv0_start = args.iter().position(|byte| *byte != 0)?;
    let args = args.get(argv0_start..)?;
    let end = args
        .iter()
        .enumerate()
        .filter(|(_, byte)| **byte == 0)
        .nth(argc as usize - 1)?
        .0;
    names_from_cmdline(args.get(..=end)?)
}

#[cfg(test)]
mod tests;
