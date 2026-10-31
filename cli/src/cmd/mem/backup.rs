use crate::{
    args::mem::MemBackupAction,
    cmd::print::{fail, print_human},
};
use codeterm_paths::instance_identity::ResolvedInstancePaths;

pub(crate) fn handle(json: bool, action: MemBackupAction) {
    match run(action) {
        Ok(value) if json => crate::cmd::print::print_json(&value),
        Ok(value) => print_human(&value),
        Err(error) => fail(1, error),
    }
}

fn run(action: MemBackupAction) -> Result<serde_json::Value, String> {
    crate::cmd::worker::register_sqlite_vec_extension();
    let identity = ResolvedInstancePaths::current().map_err(|e| e.to_string())?;
    match action {
        MemBackupAction::Create { out } => {
            let path =
                codeterm_mem::backup::create(identity.data(), &out).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({"backup": path}))
        }
        MemBackupAction::Restore { from } => {
            let home = dirs::home_dir().ok_or("Home directory unavailable")?;
            let registry = home.join(".codeterm-instance-lifetimes/path-owners");
            std::fs::create_dir_all(
                registry
                    .parent()
                    .ok_or("Ownership registry has no parent")?,
            )
            .map_err(|e| e.to_string())?;
            let previous = codeterm_mem::backup::restore(&from, identity, &registry)
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({"restored": true, "previous_data": previous}))
        }
    }
}

#[cfg(test)]
mod tests;
