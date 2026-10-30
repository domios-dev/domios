fn unquote(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.starts_with(['\'', '"']) {
        let quote = value.chars().next()?;
        value.strip_prefix(quote)?.strip_suffix(quote)
    } else {
        Some(value)
    }
    .filter(|path| !path.is_empty())
}

fn absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with('/')
        || path.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
}

fn tokens(mut argument: &str) -> Option<Vec<&str>> {
    let mut paths = Vec::new();
    while !argument.trim_start().is_empty() {
        argument = argument.trim_start();
        let (path, rest) = if argument.starts_with(['\'', '"']) {
            let quote = argument.chars().next()?;
            let after_quote = &argument[quote.len_utf8()..];
            let end = after_quote.find(quote)?;
            let rest = &after_quote[end + quote.len_utf8()..];
            if rest.chars().next().is_some_and(|c| !c.is_whitespace()) {
                return None;
            }
            (&after_quote[..end], rest)
        } else {
            let end = argument.find(char::is_whitespace).unwrap_or(argument.len());
            (&argument[..end], &argument[end..])
        };
        paths.push(path);
        argument = rest;
    }
    Some(paths)
}

pub fn artifact_paths<'a>(tag: &str, argument: &'a str) -> Vec<&'a str> {
    if tag.eq_ignore_ascii_case("@ds-image") || tag.eq_ignore_ascii_case("@ct-image") {
        if let Some(paths) = tokens(argument) {
            if paths.len() > 1 && paths.iter().all(|path| absolute(path)) {
                return paths;
            }
        }
    }
    unquote(argument).into_iter().collect()
}

#[cfg(test)]
mod tests;
