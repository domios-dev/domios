use crate::cmd::payload_source::{current_pad, list_pad_files};
use crate::PadAction;

pub fn handle_pad(json: bool, action: PadAction) {
    let result = match action {
        PadAction::Path => current_pad().map(|path| {
            if json {
                serde_json::json!({"path": path})
            } else {
                serde_json::json!(path.to_string_lossy())
            }
        }),
        PadAction::List => list_pad_files().map(|files| {
            serde_json::json!(files
                .into_iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect::<Vec<_>>())
        }),
    };
    match result {
        Ok(value) if json => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
        Ok(serde_json::Value::String(value)) => println!("{value}"),
        Ok(serde_json::Value::Array(values)) => {
            for value in values {
                println!("{}", value.as_str().unwrap_or_default());
            }
        }
        Ok(_) => unreachable!(),
        Err(error) => {
            eprintln!("Error: {error}");
            std::process::exit(1);
        }
    }
}
