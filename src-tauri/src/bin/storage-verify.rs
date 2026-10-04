use codeterm_lib::storage::verification::verify_copied_state;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;

struct Arguments {
    copy_root: PathBuf,
    work_root: PathBuf,
}

const HELP: &str = "storage-verify imports a copied Domios state root into work-root/instance, reads it through the post-cutover adapters, reopens it, and writes a finite JSON receipt.\n\nUSAGE:\n  cargo run --manifest-path src-tauri/Cargo.toml --bin storage-verify -- --copy-root <PATH> --work-root <PATH>\n\nOPTIONS:\n  --copy-root <PATH>  Inert copied instance root containing importer inventory files\n  --work-root <PATH>  Fresh separate work root; it must not overlap the copied root\n  -h, --help          Print this help\n\nCOPY PROCEDURE:\n  Copy only files enumerated by this build's importer inventory. Exclude secrets/, mesh keys, and config tokens. Create a SHA-256 manifest before transfer and verify it again after copying. The receipt includes the verifier's per-file SHA-256 manifests before and after import. Use a fresh work root.\n\nRECEIPT:\n  <work-root>/storage-verification-receipt.json\n";

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let Some(arguments) = parse_arguments(env::args_os().skip(1))? else {
        print!("{HELP}");
        return Ok(());
    };
    let report = verify_copied_state(&arguments.copy_root, &arguments.work_root)?;
    fs::create_dir_all(&arguments.work_root)
        .map_err(|error| format!("create receipt directory: {error}"))?;
    let receipt_path = arguments
        .work_root
        .join("storage-verification-receipt.json");
    let mut receipt = serde_json::to_vec_pretty(&report)
        .map_err(|error| format!("serialize verification receipt: {error}"))?;
    receipt.push(b'\n');
    fs::write(&receipt_path, receipt)
        .map_err(|error| format!("write verification receipt: {error}"))?;
    println!("receipt: {}", receipt_path.display());
    if report.passed {
        Ok(())
    } else {
        Err("verification completed with differences; inspect the JSON receipt".to_string())
    }
}

fn parse_arguments(arguments: impl Iterator<Item = OsString>) -> Result<Option<Arguments>, String> {
    let mut arguments = arguments;
    let mut copy_root = None;
    let mut work_root = None;
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("-h" | "--help") => return Ok(None),
            Some("--copy-root") => {
                copy_root = Some(PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "--copy-root requires a path".to_string())?,
                ));
            }
            Some("--work-root") => {
                work_root = Some(PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "--work-root requires a path".to_string())?,
                ));
            }
            Some(unknown) => return Err(format!("unknown option: {unknown}\n\n{HELP}")),
            None => return Err("options must be valid UTF-8".to_string()),
        }
    }
    match (copy_root, work_root) {
        (Some(copy_root), Some(work_root)) => Ok(Some(Arguments {
            copy_root,
            work_root,
        })),
        _ => Err(format!(
            "--copy-root and --work-root are required\n\n{HELP}"
        )),
    }
}
