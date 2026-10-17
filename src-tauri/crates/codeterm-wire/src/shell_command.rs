use serde::{Deserialize, Deserializer};

pub fn validate(command: &str) -> Result<(), String> {
    for (position, character) in command.chars().enumerate() {
        if character <= '\u{1f}' && !matches!(character, '\t' | '\n' | '\r') {
            return Err(format!(
                "shell command contains control character U+{:04X} at character position {} (1-based); use forward slashes or double the backslash",
                character as u32, position + 1
            ));
        }
    }
    Ok(())
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let command = String::deserialize(deserializer)?;
    validate(&command).map_err(serde::de::Error::custom)?;
    Ok(command)
}

pub fn deserialize_optional<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    let command = Option::<String>::deserialize(deserializer)?;
    if let Some(command) = &command {
        validate(command).map_err(serde::de::Error::custom)?;
    }
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_escapes_paths_and_shell_whitespace_are_unchanged() {
        for command in [
            r"\b",
            r"\n",
            r"\t",
            r".\build.ps1",
            r"C:\new\test",
            "one\ntwo\tthree\r\n",
        ] {
            assert_eq!(validate(command), Ok(()));
        }
    }

    #[test]
    fn rejects_every_other_c0_character_with_exact_position_and_hint() {
        for code in 0..=31 {
            let character = char::from_u32(code).unwrap();
            if matches!(character, '\t' | '\n' | '\r') {
                continue;
            }
            let error = validate(&format!("雪.{character}uild.ps1")).unwrap_err();
            assert!(error.contains(&format!("U+{code:04X}")));
            assert!(error.contains("position 3 (1-based)"));
            assert!(error.contains("use forward slashes or double the backslash"));
        }
    }
}
