use std::path::Path;

use crate::cmd::payload_source::{current_pad, display_pad_path, list_pad_files};
use crate::PadAction;

fn pad_path_value(path: &Path, json: bool) -> serde_json::Value {
    let display = display_pad_path(path);
    if json {
        serde_json::json!({ "path": display })
    } else {
        serde_json::json!(display)
    }
}

pub fn handle_pad(json: bool, action: PadAction) {
    let result = match action {
        PadAction::Path => current_pad().map(|path| pad_path_value(&path, json)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn pad_path_hides_the_verbatim_prefix_in_both_output_modes() {
        let verbatim = PathBuf::from(r"\\?\D:\Developer\codeterm\pads\tab_1");
        let expected = r"D:\Developer\codeterm\pads\tab_1";
        assert_eq!(pad_path_value(&verbatim, false), serde_json::json!(expected));
        assert_eq!(
            pad_path_value(&verbatim, true),
            serde_json::json!({ "path": expected })
        );
    }

    #[test]
    fn pad_path_restores_the_unc_share_form() {
        let verbatim = PathBuf::from(r"\\?\UNC\server\share\pads\tab_1");
        let expected = r"\\server\share\pads\tab_1";
        assert_eq!(pad_path_value(&verbatim, false), serde_json::json!(expected));
        assert_eq!(
            pad_path_value(&verbatim, true),
            serde_json::json!({ "path": expected })
        );
    }

    #[test]
    fn pad_path_leaves_an_already_plain_path_untouched() {
        let plain = PathBuf::from("/home/user/.codeterm/pads/tab_1");
        assert_eq!(
            pad_path_value(&plain, true),
            serde_json::json!({ "path": "/home/user/.codeterm/pads/tab_1" })
        );
    }
}
