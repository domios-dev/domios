// Caller: scripts dev-prep.mjs
//! One-off plan storage migration entry point.
//!
//! Rehearse on a COPY first — never run live against the only checkout.
//!
//! ```bash
//! cargo run -p codeterm --bin convert-plan-storage -- --project /path/to/copy --dry-run
//! ```

fn main() {
    if let Err(error) = codeterm_lib::service::plan::convert_storage::main() {
        eprintln!("convert-plan-storage: {error}");
        std::process::exit(1);
    }
}
