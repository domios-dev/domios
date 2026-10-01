//! `mem graph hubs`: the capped, clustered hub report, read from the local DB.

use codeterm_mem_store::hubs::{hub_report, HubQuery};

use crate::cmd::graph_mutate::{open_conn, print_or_exit};

pub fn hubs_json(
    conn: &rusqlite::Connection,
    query: HubQuery,
) -> Result<serde_json::Value, String> {
    let report = hub_report(conn, query).map_err(|e| e.to_string())?;
    serde_json::to_value(report).map_err(|e| e.to_string())
}

pub fn run_hubs(threshold: u32, limit: u32, sample: u32, db_path: Option<&str>) -> ! {
    let query = HubQuery {
        threshold,
        limit,
        sample,
    };
    print_or_exit(open_conn(db_path).and_then(|conn| hubs_json(&conn, query)));
}

#[cfg(test)]
mod tests;
