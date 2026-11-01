use url::Url;

#[derive(Debug, PartialEq, Eq)]
pub struct DevelopmentFrontend {
    pub origin: Url,
    pub identifier: String,
}

fn runtime_port(value: &str) -> Result<u16, String> {
    value
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| "Invalid development runtime port".into())
}

pub fn development_runtime_frontend(
    debug: bool,
    development: bool,
    port: &str,
    identifier: &str,
) -> Result<Option<DevelopmentFrontend>, String> {
    if !debug || !development || port.is_empty() {
        return Ok(None);
    }
    let port = runtime_port(port)?;
    if !(identifier == "com.domios.dev" || identifier.starts_with("com.domios.dev."))
        || !identifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".-".contains(&byte))
    {
        return Err("Invalid development frontend identifier".into());
    }
    Ok(Some(DevelopmentFrontend {
        origin: Url::parse(&format!("http://127.0.0.1:{port}"))
            .map_err(|error| error.to_string())?,
        identifier: identifier.into(),
    }))
}

pub fn development_browser_arguments(port: &str, inherited: &str) -> Result<String, String> {
    let port = runtime_port(port)?;
    let mut arguments = inherited.to_string();
    if !arguments.contains("--remote-debugging-port=") {
        if !arguments.is_empty() {
            arguments.push(' ');
        }
        arguments.push_str(&format!("--remote-debugging-port={port}"));
    }
    if !arguments.contains("--disable-features=") {
        arguments =
            format!("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection {arguments}");
    }
    Ok(arguments)
}

#[cfg(test)]
mod tests;
