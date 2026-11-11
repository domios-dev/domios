use serde_json::json;

use crate::args::team::exchange::{DistillArgs, ExportArgs, ImportArgs};
use crate::client::RpcClient;

fn absolute(path: &str) -> String {
    std::path::absolute(path)
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| path.to_string())
}

pub fn distill(client: &mut dyn RpcClient, json_output: bool, args: DistillArgs) {
    let params = json!({
        "team_id": args.team_id,
        "format": args.format,
        "out": args.out.as_deref().map(absolute),
    });
    super::finish(
        json_output,
        client.call_detailed("team.distill", Some(params)),
    );
}

pub fn export(client: &mut dyn RpcClient, json_output: bool, args: ExportArgs) {
    let params = json!({
        "template": super::template_start::template_ref(&args.name),
        "format": args.format,
        "out": args.out.as_deref().map(absolute),
    });
    super::finish(
        json_output,
        client.call_detailed("team.export", Some(params)),
    );
}

pub fn import(client: &mut dyn RpcClient, json_output: bool, args: ImportArgs) {
    let params = json!({ "path": absolute(&args.path) });
    super::finish(
        json_output,
        client.call_detailed("team.import", Some(params)),
    );
}
