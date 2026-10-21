pub(super) fn valid_distro(distro: &str) -> bool {
    !distro.is_empty()
        && !distro
            .chars()
            .any(|c| matches!(c, '/' | '\\' | '\0' | ':') || c.is_control())
        && distro != "."
        && distro != ".."
}

pub fn windows_to_wsl(path: &str, distro: &str) -> Result<String, String> {
    if !valid_distro(distro) || path.contains('\0') {
        return Err("Invalid WSL distribution or working directory".into());
    }
    let path = codeterm_paths::strip_verbatim_prefix(path);
    let normalized = path.replace('\\', "/");
    if normalized.starts_with("//") {
        for host in ["wsl.localhost", "wsl$"] {
            let prefix = format!("//{host}/{distro}");
            if normalized
                .get(..prefix.len())
                .is_some_and(|p| p.eq_ignore_ascii_case(&prefix))
            {
                let rest = &normalized[prefix.len()..];
                if rest.is_empty() || rest.starts_with('/') {
                    return Ok(if rest.is_empty() {
                        "/".into()
                    } else {
                        rest.into()
                    });
                }
            }
        }
        return Err(format!(
            "Path is not accessible in WSL distribution {distro}: {path}"
        ));
    }
    let bytes = normalized.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/' {
        return Ok(format!(
            "/mnt/{}/{}",
            (bytes[0] as char).to_ascii_lowercase(),
            &normalized[3..]
        ));
    }
    if normalized.starts_with('/') {
        return Ok(normalized);
    }
    Err(format!(
        "WSL requires an absolute working directory: {path}"
    ))
}

pub fn wsl_to_windows(path: &str, distro: &str) -> Result<String, String> {
    if !valid_distro(distro) || !path.starts_with('/') || path.contains('\0') {
        return Err("Invalid WSL working directory".into());
    }
    if let Some(rest) = path.strip_prefix("/mnt/") {
        let bytes = rest.as_bytes();
        if !bytes.is_empty()
            && bytes[0].is_ascii_alphabetic()
            && (bytes.len() == 1 || bytes[1] == b'/')
        {
            let suffix = rest.get(2..).unwrap_or_default().replace('/', "\\");
            return Ok(format!(
                "{}:\\{suffix}",
                (bytes[0] as char).to_ascii_uppercase()
            ));
        }
    }
    Ok(format!(
        "\\\\wsl.localhost\\{distro}{}",
        path.replace('/', "\\")
    ))
}
