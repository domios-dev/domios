use super::RuntimeLaunchProfile;

pub fn exact_npm(spec: &str) -> bool {
    let Some((package, version)) = spec.rsplit_once('@') else {
        return false;
    };
    let name = package.strip_prefix('@').unwrap_or(package);
    !name.is_empty()
        && name.len() <= 214
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-/".contains(&b))
        && name
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && name.matches('/').count() == usize::from(package.starts_with('@'))
        && semver::Version::parse(version).is_ok()
}

pub fn exact_python(spec: &str) -> Option<(&str, &str)> {
    let (name, version) = spec.split_once("==")?;
    if name.is_empty()
        || name.len() > 214
        || !name.as_bytes()[0].is_ascii_alphanumeric()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        || version.is_empty()
        || version.len() > 128
        || !version.as_bytes()[0].is_ascii_digit()
        || !version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".!+_-".contains(&b))
    {
        return None;
    }
    Some((name, version))
}

pub fn https(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
            && url.port().is_none()
    })
}

pub fn sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn relative_executable(value: &str) -> bool {
    let value = value.strip_prefix("./").unwrap_or(value);
    !value.is_empty()
        && value.len() <= 1024
        && !value.starts_with('-')
        && !value.contains(['\\', ':', '*', '?', '[', ']'])
        && !value.chars().any(char::is_control)
        && value
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && !reserved_name(part))
}

pub fn reserved_name(value: &str) -> bool {
    let stem = value.split('.').next().unwrap_or("").to_ascii_uppercase();
    [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ]
    .contains(&stem.as_str())
}

pub fn validate(profile: &RuntimeLaunchProfile) -> bool {
    let Some(install) = &profile.install else {
        let basename = profile
            .binary
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("")
            .split('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        return !["npx", "npm", "uvx", "uv", "env"].contains(&basename.as_str());
    };
    match install.distribution.as_deref().unwrap_or("npx") {
        "npx" => {
            exact_npm(&install.package)
                && profile.binary == "npx"
                && profile.args.first() == Some(&install.package)
                && install
                    .registry
                    .as_deref()
                    .is_none_or(|r| r == "https://registry.npmjs.org")
                && install.url.is_none()
                && install.executable.is_none()
                && install.sha256.is_none()
        }
        "uvx" => {
            exact_python(&install.package).is_some()
                && profile.binary == "uvx"
                && profile.args.first() == Some(&install.package)
                && install.registry.is_none()
                && install.url.is_none()
                && install.executable.is_none()
                && install.sha256.is_none()
        }
        "binary" => {
            exact_npm(&install.package)
                && install.registry.is_none()
                && install.url.as_deref().is_some_and(https)
                && install.sha256.as_deref().is_some_and(sha256)
                && install
                    .executable
                    .as_deref()
                    .is_some_and(relative_executable)
                && install.executable.as_deref() == Some(profile.binary.as_str())
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests;
