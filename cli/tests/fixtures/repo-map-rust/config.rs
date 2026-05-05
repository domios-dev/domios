pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

pub struct Config {
    pub level: LogLevel,
    pub port: u16,
}

pub fn default_config() -> Config {
    Config {
        level: LogLevel::Info,
        port: 8080,
    }
}

pub fn load_config(path: &str) -> Result<Config, String> {
    let _ = path;
    Ok(default_config())
}
