use clap::Subcommand;
use serde_json::Value;

use crate::{build_local_api, local_api::LocalApiClient, read_api_config};

#[derive(Subcommand, Debug)]
pub enum MeshCloudAction {
    /// Create and enroll a new managed cloud machine
    Create {
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
    /// Show managed cloud credit usage, allowance, remaining balance, cap, and burn per size
    Usage,
    /// Redeem a cloud credit promo code into your account balance
    Redeem {
        /// Promo code to redeem
        code: String,
    },
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
    match rate["credits_per_hour"].as_f64() {
        Some(credits) if credits.is_finite() && credits > 0.0 => {
            format!("{amount:.2} {currency}/h ({credits:.2} cr/h)")
        }
        _ => format!("{amount:.2} {currency}/h"),
    }
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

pub(crate) fn format_usage_human(val: &Value) -> String {
    let used = val["used"].as_f64().unwrap_or(0.0);
    let allowance = val["allowance"].as_f64().unwrap_or(0.0);
    let purchased = val["purchased"].as_f64().unwrap_or(0.0);
    let remaining = val["remaining"].as_f64().unwrap_or(0.0);
    let cap = val["cap"].as_f64().unwrap_or(0.0);
    let mut lines = vec![
        format!(
            "Cloud credits: used={used:.2}h remaining={remaining:.2}h (allowance={allowance:.2}h purchased={purchased:.2}h cap={cap:.2})"
        ),
    ];
    if let Some(burn) = val["burn_per_size"].as_object().filter(|m| !m.is_empty()) {
        let mut entries: Vec<String> = burn
            .iter()
            .filter_map(|(size, rate)| rate.as_f64().map(|r| format!("{size}={r:.2} cr/h")))
            .collect();
        entries.sort();
        if !entries.is_empty() {
            lines.push(format!("Burn per size: {}", entries.join(", ")));
        }
    }
    lines.join("\n")
}

pub(crate) fn format_redeem_human(val: &Value) -> String {
    let hours = val["hours"].as_f64().unwrap_or(0.0);
    let remaining = val["remaining"].as_f64().unwrap_or(0.0);
    let purchased = val["purchased"].as_f64().unwrap_or(0.0);
    let already = val["already_redeemed"].as_bool().unwrap_or(false);
    let lead = if already {
        format!("Promo code already redeemed (+{hours:.2}h)")
    } else {
        format!("Redeemed promo code: +{hours:.2}h cloud credits")
    };
    let expiry = match val["expires_at"].as_u64() {
        Some(ts) => format!(" expires_at={ts}"),
        None => " expires=never".into(),
    };
    format!("{lead}{expiry} (remaining={remaining:.2}h purchased={purchased:.2}h)")
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
    let is_usage = matches!(action, MeshCloudAction::Usage);
    let is_redeem = matches!(action, MeshCloudAction::Redeem { .. });
    let response = match action {
        MeshCloudAction::Create { size, name } => {
            let mut body = serde_json::json!({});
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
        MeshCloudAction::Redeem { code } => http
            .post(format!("{base}/api/mesh/cloud/redeem"))
            .header("Authorization", auth_header(token))
            .json(&serde_json::json!({ "code": code }))
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
            } else if is_usage {
                println!("{}", format_usage_human(&val));
            } else if is_redeem {
                println!("{}", format_redeem_human(&val));
            } else if let Some(machines) = val["machines"].as_array() {
                if machines.is_empty() {
                    println!("No cloud machines.");
                } else {
                    for machine in machines {
                        println!("{}", format_receipt_human(machine));
                    }
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
