pub const PRODUCTION_PORT: u16 = 7685;

pub fn validate_listener_port(port: u16, development: bool) -> Result<(), String> {
    if development && port == PRODUCTION_PORT {
        return Err(format!(
            "development/debug builds refuse production port {port}; choose a private CODETERM_API_PORT"
        ));
    }
    Ok(())
}

pub fn runtime_api_port(
    configured: u16,
    environment: Option<&str>,
    development: bool,
) -> Result<u16, String> {
    validate_listener_port(configured, development)
        .map_err(|error| format!("{error}; change [api].port in the development config.toml"))?;
    let port = if development {
        match environment {
            Some(value) => value
                .trim()
                .parse::<u16>()
                .map_err(|_| "CODETERM_API_PORT must be a valid TCP port".to_string())?,
            None => configured,
        }
    } else {
        configured
    };
    if port == 0 {
        return Err("the instance API port must be nonzero".to_string());
    }
    validate_listener_port(port, development)?;
    Ok(port)
}

#[cfg(test)]
mod tests;
