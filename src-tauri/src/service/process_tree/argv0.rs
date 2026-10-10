fn basename(bytes: &[u8]) -> Option<String> {
    let name = std::str::from_utf8(bytes).ok()?.rsplit('/').next()?;
    (!name.is_empty()).then(|| name.to_owned())
}

pub(super) fn from_cmdline(bytes: &[u8]) -> Option<String> {
    basename(bytes.get(..bytes.iter().position(|byte| *byte == 0)?)?)
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn from_procargs(bytes: &[u8]) -> Option<String> {
    let argc = i32::from_ne_bytes(bytes.get(..4)?.try_into().ok()?);
    if argc <= 0 {
        return None;
    }
    let bytes = bytes.get(4..)?;
    let executable_end = bytes.iter().position(|byte| *byte == 0)?;
    let args = bytes.get(executable_end + 1..)?;
    let argv0_start = args.iter().position(|byte| *byte != 0)?;
    from_cmdline(args.get(argv0_start..)?)
}

#[cfg(test)]
mod tests;
