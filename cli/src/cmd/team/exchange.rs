use serde_json::json;

use crate::args::team::exchange::{DeleteArgs, DistillArgs, ExportArgs, ImportArgs, RecallArgs};
use crate::client::RpcClient;

fn absolute(path: &str) -> String {
    std::path::absolute(path)
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| path.to_string())
}

pub fn distill(client: &mut dyn RpcClient, json_output: bool, args: DistillArgs) {
    let runbook = match args.runbook_file.as_deref().map(std::fs::read_to_string) {
        Some(Ok(text)) => Some(text),
        Some(Err(error)) => {
            eprintln!("Error: cannot read the runbook file: {error}");
            std::process::exit(1);
        }
        None => None,
    };
    let params = json!({
        "team_id": args.team_id,
        "name": args.name,
        "version": args.version,
        "description": args.description,
        "title": args.title,
        "license": args.license,
        "when": args.when,
        "runbook": runbook,
        "opt_out": args.opt_out,
        "format": args.format,
        "out": args.out.as_deref().map(absolute),
        "save": args.save,
        "dry_run": args.dry_run,
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

pub fn recall(client: &mut dyn RpcClient, json_output: bool, args: RecallArgs) {
    let params = json!({ "query": args.query });
    super::finish(
        json_output,
        client.call_detailed("team.template.recall", Some(params)),
    );
}

pub fn delete(client: &mut dyn RpcClient, json_output: bool, args: DeleteArgs) {
    let params = json!({ "template": super::template_start::template_ref(&args.name) });
    super::finish(
        json_output,
        client.call_detailed("team.template.delete", Some(params)),
    );
}

pub fn import(client: &mut dyn RpcClient, json_output: bool, args: ImportArgs) {
    let params = json!({ "path": absolute(&args.path) });
    super::finish(
        json_output,
        client.call_detailed("team.import", Some(params)),
    );
}
