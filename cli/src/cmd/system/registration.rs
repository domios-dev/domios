use std::path::PathBuf;

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn xml_element<'a>(raw: &'a str, tag: &str) -> Option<&'a str> {
    let pattern = format!(r"(?s)<{tag}>(.*?)</{tag}>");
    let regex = regex::Regex::new(&pattern).ok()?;
    Some(regex.captures(raw)?.get(1)?.as_str())
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn xml_text(raw: &str, tag: &str) -> Option<String> {
    let value = xml_element(raw, tag)?;
    Some(
        value
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&"),
    )
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn macos_target(plist: &str) -> Option<PathBuf> {
    let (_, arguments) = plist.split_once("<key>ProgramArguments</key>")?;
    let arguments = xml_element(arguments, "array")?;
    let target = xml_text(arguments, "string")?;
    let expected = ["system", "supervise"];
    let mut remaining = arguments.split_once("</string>")?.1;
    for argument in expected {
        if xml_text(remaining, "string")? != argument {
            return None;
        }
        remaining = remaining.split_once("</string>")?.1;
    }
    Some(target.into())
}

#[cfg(any(target_os = "windows", test))]
pub(super) fn windows_target(task: &str) -> Option<PathBuf> {
    let action = xml_element(task, "Exec")?;
    let command = xml_text(action, "Command")?;
    let arguments = xml_text(action, "Arguments")?;
    if arguments.starts_with("system supervise ") {
        return Some(command.into());
    }
    if !command.eq_ignore_ascii_case("powershell.exe") {
        return None;
    }
    let regex = regex::Regex::new(
        r"& '((?:[^']|'')*)' system supervise --binary ",
    ).ok()?;
    Some(
        regex
            .captures(&arguments)?
            .get(1)?
            .as_str()
            .replace("''", "'")
            .into(),
    )
}

#[cfg(any(target_os = "windows", test))]
pub(super) fn windows_task_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[b'<', 0]) {
        let bytes = bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(bytes);
        let words: Vec<_> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|word| u16::from_le_bytes(*word))
            .collect();
        String::from_utf16_lossy(&words)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn linux_target(unit: &str) -> Option<PathBuf> {
    let command = unit
        .lines()
        .find_map(|line| line.strip_prefix("ExecStart="))?;
    let mut chars = command.chars();
    let mut path = String::new();
    let quoted = command.starts_with('"');
    if quoted {
        chars.next();
    }
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => path.push(chars.next()?),
            '"' if quoted => break,
            ' ' if !quoted => break,
            _ => path.push(ch),
        }
    }
    (chars.as_str().trim_start().starts_with("system supervise ") && !path.is_empty())
        .then(|| path.into())
}

#[cfg(test)]
mod tests;
