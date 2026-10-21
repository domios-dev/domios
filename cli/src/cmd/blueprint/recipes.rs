use super::recipe_files::FILES;

pub(super) fn print(json_mode: bool, name: Option<&str>) {
    if let Some(name) = name {
        let raw = super::authoring::scaffold(name)
            .unwrap_or_else(|error| crate::cmd::print::fail(2, error));
        if json_mode {
            crate::cmd::print::print_json(&serde_json::json!({"name":name,"yaml":raw}));
        } else {
            print!("{raw}");
        }
    } else if json_mode {
        crate::cmd::print::print_json(&serde_json::json!(FILES
            .iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>()));
    } else {
        for (name, _) in FILES {
            println!("{name}");
        }
    }
}

#[cfg(test)]
mod tests;
