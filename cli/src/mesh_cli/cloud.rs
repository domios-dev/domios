use clap::Subcommand;
use serde_json::Value;

use crate::{build_local_api, local_api::LocalApiClient, read_api_config};

#[derive(Subcommand, Debug)]
pub enum MeshCloudAction {
    /// Create and enroll a new cloud machine through a configured cloud provider plugin
    Create {
        /// Cloud provider plugin ID (default: the sole installed cloud provider)
        #[arg(long)]
        provider: Option<String>,
        /// Machine size profile (default: small)
        #[arg(long)]
        size: Option<String>,
        /// Human-friendly machine name
        #[arg(long)]
        name: Option<String>,
    },
    /// List cloud machines with state, hourly rate, and uptime
    List,
    /// Drain an idle cloud machine and put it to sleep (refuses while work runs)
    Sleep {
        /// Cloud machine ID, name, or peer ID
        machine: String,
    },
    /// Wake a sleeping cloud machine (concurrent wakes join one wake)
    Wake {
        /// Cloud machine ID, name, or peer ID
        machine: String,
    },
    /// Permanently delete a cloud machine (warns about unpushed work without --confirm)
    Delete {
        /// Cloud machine ID, name, or peer ID
        machine: String,
        /// Confirm permanent deletion of the machine and any unpushed work
        #[arg(long)]
        confirm: bool,
    },
    /// Show cloud usage summary (managed credit accounting reserved for Tally)
    Usage,
}

fn auth_header(token: &Option<String>) -> String {
    format!("Bearer {}", token.as_deref().unwrap_or(""))
}

fn format_uptime(secs: u64) -> String {
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let rem = secs % 60;
    if hours > 0 {
        format!("{hours}h{mins:02}m")
    } else if mins > 0 {
        format!("{mins}m{rem:02}s")
    } else {
        format!("{rem}s")
    }
}

pub(crate) fn format_rate(rate: Option<&Value>) -> String {
    let Some(rate) = rate.filter(|v| v.is_object()) else {
        return "rate=unknown".into();
    };
    let amount = rate["amount"].as_f64().unwrap_or(0.0);
    let currency = rate["currency"].as_str().unwrap_or("USD");
    let basis = rate["basis"].as_str().unwrap_or("estimate");
    format!("{amount:.2} {currency}/h ({basis})")
}

pub(crate) fn format_cloud_cost_badge(cloud: &Value) -> String {
    let provider = cloud["provider"].as_str().unwrap_or("?");
    let size = cloud["size"].as_str().unwrap_or("small");
    let state = cloud["state"].as_str().unwrap_or("?");
    let uptime = format_uptime(cloud["uptime_secs"].as_u64().unwrap_or(0));
    let rate = format_rate(cloud.get("rate_per_hour"));
    format!("cloud {provider}/{size} {rate} state={state} uptime={uptime}")
}

pub(crate) fn format_receipt_human(val: &Value) -> String {
    let id = val["machine_id"].as_str().unwrap_or("?");
    let name = val["name"].as_str().unwrap_or(id);
    let badge = format_cloud_cost_badge(val);
    let notice = val["billing_notice"]
        .as_str()
        .unwrap_or("This machine costs money while awake.");
    let mut lines = vec![
        format!("{name} ({id}) — {badge}"),
        format!("note: {notice}"),
    ];
    if let Some(warning) = val["warning"].as_str().filter(|w| !w.is_empty()) {
        lines.push(format!("WARNING: {warning}"));
    }
    lines.join("\n")
}

pub(crate) fn resolve_cloud_target(target: &str) -> Result<(String, Value), String> {
    let (base, token) = read_api_config();
    let http =
        build_local_api().map_err(|error| format!("failed to build HTTP client: {error}"))?;
    let resp = http
        .post(format!("{base}/api/mesh/cloud/resolve"))
        .header("Authorization", auth_header(&token))
        .json(&serde_json::json!({ "target": target }))
        .send()
        .map_err(|error| error.to_string())?;
    let status = resp.status();
    let body = resp.text().unwrap_or_default();
    if !status.is_success() {
        return Err(format!("HTTP {} — {}", status.as_u16(), body.trim()));
    }
    let val: Value = serde_json::from_str(&body).map_err(|error| format!("Bad JSON: {error}"))?;
    let peer = val["peer"]
        .as_str()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| "cloud resolve returned no peer target".to_owned())?
        .to_owned();
    Ok((peer, val["machine"].clone()))
}

pub fn handle_cloud(
    http: &LocalApiClient,
    base: &str,
    token: &Option<String>,
    json: bool,
    action: MeshCloudAction,
) {
    let response = match action {
        MeshCloudAction::Create {
            provider,
            size,
            name,
        } => {
            let mut body = serde_json::json!({});
            if let Some(provider) = provider {
                body["provider"] = serde_json::json!(provider);
            }
            if let Some(size) = size {
                body["size"] = serde_json::json!(size);
            }
            if let Some(name) = name {
                body["name"] = serde_json::json!(name);
            }
            http.post(format!("{base}/api/mesh/cloud/create"))
                .header("Authorization", auth_header(token))
                .json(&body)
                .send()
        }
        MeshCloudAction::List => http
            .get(format!("{base}/api/mesh/cloud/list"))
            .header("Authorization", auth_header(token))
            .send(),
        MeshCloudAction::Sleep { machine } => http
            .post(format!(
                "{base}/api/mesh/cloud/{}/sleep",
                urlencoding::encode(&machine)
            ))
            .header("Authorization", auth_header(token))
            .json(&serde_json::json!({}))
            .send(),
        MeshCloudAction::Wake { machine } => http
            .post(format!(
                "{base}/api/mesh/cloud/{}/wake",
                urlencoding::encode(&machine)
            ))
            .header("Authorization", auth_header(token))
            .json(&serde_json::json!({}))
            .send(),
        MeshCloudAction::Delete { machine, confirm } => http
            .post(format!(
                "{base}/api/mesh/cloud/{}/delete",
                urlencoding::encode(&machine)
            ))
            .header("Authorization", auth_header(token))
            .json(&serde_json::json!({ "confirm": confirm }))
            .send(),
        MeshCloudAction::Usage => http
            .get(format!("{base}/api/mesh/cloud/usage"))
            .header("Authorization", auth_header(token))
            .send(),
    };

    match response {
        Ok(resp) if resp.status().is_success() => {
            let val: Value = match resp.json() {
                Ok(val) => val,
                Err(error) => crate::cmd::print::fail(1, format!("Bad JSON: {error}")),
            };
            if json {
                crate::cmd::print::print_json(&val);
            } else if let Some(machines) = val["machines"].as_array() {
                if machines.is_empty() {
                    println!("No cloud machines.");
                } else {
                    for machine in machines {
                        println!("{}", format_receipt_human(machine));
                    }
                }
                if let Some(limit) = val["limit"].as_u64() {
                    println!("Cloud machine limit: {}/{}", machines.len(), limit);
                }
                if let Some(total) = val["total_uptime_secs"].as_u64() {
                    println!("Total period uptime: {}", format_uptime(total));
                }
            } else {
                println!("{}", format_receipt_human(&val));
            }
        }
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            crate::cmd::print::fail_http(1, status.as_u16(), &body);
        }
        Err(error) => crate::cmd::print::fail(1, error.to_string()),
    }
}

#[cfg(test)]
#[path = "cloud_tests.rs"]
mod tests;
