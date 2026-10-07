use codeterm_types::agent::chat_types::ChatMsg;
use ts_rs::{Config, TS};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env().with_large_int("number");
    ChatMsg::export_all(&config)?;
    Ok(())
}
