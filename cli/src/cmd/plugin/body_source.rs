use crate::cmd::payload_source;

pub(super) fn resolve(args: Vec<String>) -> Result<(Vec<String>, Option<String>), String> {
    resolve_with(
        args,
        payload_source::read_from_pad,
        payload_source::read_file_or_stdin,
    )
}

fn resolve_with(
    args: Vec<String>,
    read_pad: impl FnOnce(&str) -> Result<String, String>,
    read_file: impl FnOnce(&str, &str) -> Result<String, String>,
) -> Result<(Vec<String>, Option<String>), String> {
    let mut remaining = Vec::with_capacity(args.len());
    let mut source = None;
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        if arg == "--" {
            remaining.push(arg);
            remaining.extend(iter);
            break;
        }
        if !matches!(arg.as_str(), "--from-pad" | "--from-file" | "--stdin") {
            remaining.push(arg);
            continue;
        }
        if source.is_some() {
            return Err("choose only one body source: --from-pad, --from-file, or --stdin".into());
        }
        let path = if arg == "--stdin" {
            "-".to_string()
        } else {
            iter.next()
                .filter(|path| !path.is_empty() && !path.starts_with("--"))
                .ok_or_else(|| format!("{arg} requires a source path (or - for stdin)"))?
        };
        source = Some((arg, path));
    }
    let body = match source {
        Some((option, path)) if option == "--from-pad" => Some(read_pad(&path)?),
        Some((option, path)) => Some(read_file(&path, &option)?),
        None => None,
    };
    Ok((remaining, body))
}

#[cfg(test)]
mod tests;
