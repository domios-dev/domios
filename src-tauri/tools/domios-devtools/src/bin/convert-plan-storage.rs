// Caller: scripts dev-prep.mjs
//! One-off plan storage migration entry point.
//!
//! Rehearse on a COPY first — never run live against the only checkout.
//!
//! ```bash
//! cargo run -p domios-devtools --features dev-tools --bin convert-plan-storage -- --project /path/to/copy --dry-run
//! ```

fn main() {
    if let Err(error) = codeterm_service_core::service::plan::convert_storage::main() {
        eprintln!("convert-plan-storage: {error}");
        std::process::exit(1);
    }
}

extern crate domios_service_core as codeterm_service_core; // rename-bridge
