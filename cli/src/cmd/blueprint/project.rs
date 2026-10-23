use crate::args::blueprint::BlueprintProjectAction;
use codeterm_blueprint_types::blueprint_project;
use serde_json::Value;

pub(super) fn read(action: &BlueprintProjectAction) -> Result<Value, String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    match action {
        BlueprintProjectAction::List => serde_json::to_value(blueprint_project::list(&root)?),
        BlueprintProjectAction::Show { path } => {
            serde_json::to_value(blueprint_project::show(&root, path)?)
        }
    }
    .map_err(|e| e.to_string())
}
