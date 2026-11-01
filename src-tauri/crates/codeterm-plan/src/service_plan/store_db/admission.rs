use super::{db_error, PlanError};
use rusqlite::Connection;

type Schema = Vec<(String, String, String, Option<String>)>;

pub(in crate::service_plan) fn schema(connection: &Connection) -> rusqlite::Result<Schema> {
    let mut statement = connection.prepare(
        "SELECT type, name, tbl_name, sql FROM sqlite_schema
         WHERE name NOT GLOB 'sqlite_*' ORDER BY type, name",
    )?;
    let rows = statement.query_map([], |row| {
        let sql: Option<String> = row.get(3)?;
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, sql.map(normalize)))
    })?;
    rows.collect()
}

pub(in crate::service_plan) fn validate(
    connection: &Connection,
    expected: &Connection,
) -> Result<(), PlanError> {
    if schema(connection).map_err(db_error)? != schema(expected).map_err(db_error)? {
        return Err(PlanError::Io(
            "unrecognized, partial or mixed plan database schema".into(),
        ));
    }
    Ok(())
}

fn normalize(sql: String) -> String {
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut characters = sql.chars().peekable();
    while let Some(character) = characters.next() {
        if character.is_alphanumeric() || matches!(character, '_' | '$') {
            word.push(character);
            continue;
        }
        if !word.is_empty() {
            tokens.push(std::mem::take(&mut word));
        }
        if character.is_ascii_whitespace() {
            continue;
        }
        let mut token = character.to_string();
        if matches!(character, '\'' | '"' | '`' | '[') {
            let end = if character == '[' { ']' } else { character };
            while let Some(quoted) = characters.next() {
                token.push(quoted);
                if quoted == end {
                    if characters.peek() == Some(&end) {
                        token.push(characters.next().expect("peeked quote"));
                    } else {
                        break;
                    }
                }
            }
        }
        tokens.push(token);
    }
    if !word.is_empty() {
        tokens.push(word);
    }
    tokens.join("\0")
}

#[cfg(test)]
mod tests;
