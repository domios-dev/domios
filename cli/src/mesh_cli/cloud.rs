use clap::Subcommand;
use serde_json::Value;

use crate::{build_local_api, local_api::LocalApiClient, read_api_config};

#[derive(Subcommand, Debug)]
pub enum MeshCloudAction {
    /// Create and enroll a new managed cloud machine
    #[command(hide = true)]
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
    /// Archive an asleep or idle cloud machine's disk now (keeps sessions, settings, memory, and repo state free of charge)
    Archive {
        /// Cloud machine ID, name, or peer ID
        machine: String,
    },
    /// Wake a sleeping cloud machine (concurrent wakes join one wake)
    Wake {
        /// Cloud machine ID, name, or peer ID
        machine: String,
    },
    /// Delete a cloud machine (permanent by default; pass --recoverable to retain verified archive)
    Delete {
        /// Cloud machine ID, name, or peer ID
        machine: String,
        /// Confirm deletion of the machine
        #[arg(long)]
        confirm: bool,
        /// Retain the verified encrypted essentials archive for recreate within the purge window
        #[arg(long)]
        recoverable: bool,
        /// Verified archive ID for recoverable deletion
        #[arg(long)]
        archive_id: Option<String>,
    },
    /// Recreate a cold or recoverably deleted cloud machine onto a fresh volume from its archive
    Recreate {
        /// Cloud machine ID, name, or peer ID
        machine: String,
        /// Optional archive ID override
        #[arg(long)]
        archive_id: Option<String>,
    },
    /// Increase a cloud machine's hot volume capacity (grow-only, requires owner --confirm)
    ResizeVolume {
        /// Cloud machine ID, name, or peer ID
        machine: String,
        /// Target volume size in GB (must exceed current allocation, max 500)
        #[arg(long)]
        size_gb: u32,
        /// Confirm volume expansion and increased hot storage billing
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
    /// Create a checkout link to top up managed cloud credits
    #[command(group(
        clap::ArgGroup::new("topup_target")
            .required(true)
            .multiple(false)
            .args(["pack", "amount"])
    ))]
    Topup {
        /// Credit pack ID from `domios mesh cloud usage`
        #[arg(long)]
        pack: Option<String>,
        /// Custom top-up amount in USD
        #[arg(long)]
        amount: Option<String>,
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
    if let Some(warm) = val.get("warm_bootstrap").filter(|v| v.is_object()) {
        let status = warm["status"].as_str().unwrap_or("unknown");
        let readable = warm["readable_owner_context"].as_bool().unwrap_or(false);
        let delta = val
            .pointer("/create_phases/warm_bootstrap_delta_secs")
            .and_then(Value::as_f64)
            .map(|secs| format!(" delta={secs:.2}s"))
            .unwrap_or_default();
        let signin_needed = warm["providers"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter(|p| p["state"].as_str() == Some("sign_in_required"))
                    .count()
            })
            .unwrap_or(0);
        lines.push(format!(
            "warm_bootstrap={status} readable_context={readable}{delta} providers_needing_signin={signin_needed}"
        ));
    }
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
    if let Some(storage) = val["storage"].as_object() {
        let alloc = storage["total_allocated_gb"].as_u64().unwrap_or(0);
        let monthly_cents = storage["total_hot_monthly_cost_cents"]
            .as_u64()
            .unwrap_or(0);
        let rate_cents = storage["hot_gb_month_rate_cents"].as_u64().unwrap_or(30);
        let hot_days = storage["hot_days"].as_f64().unwrap_or(1.0);
        let auto_archive_label = if (hot_days - 1.0).abs() < f64::EPSILON {
            "auto-archives after 1 day asleep".to_owned()
        } else if hot_days.fract() == 0.0 {
            format!("auto-archives after {} days asleep", hot_days as u64)
        } else {
            format!("auto-archives after {hot_days} days asleep")
        };
        lines.push(format!(
            "Cloud storage: allocated={alloc} GB hot_monthly=${:.2} (rate=${:.2}/GB-mo, {auto_archive_label}, cold archival included)",
            (monthly_cents as f64) / 100.0,
            (rate_cents as f64) / 100.0
        ));
        if let Some(machines) = storage["machines"].as_array() {
            for m in machines {
                let mid = m["machine_id"].as_str().unwrap_or("?");
                let tier = match m["tier"].as_str().unwrap_or("hot") {
                    "cold" | "recoverable_deleted" => "Archived - wakes in ~2 min",
                    "evicting" => "Archiving",
                    "purged" => "Purged",
                    _ => "Hot",
                };
                let mgb = m["allocated_gb"].as_u64().unwrap_or(0);
                let used = m["used_gb"]
                    .as_f64()
                    .map(|g| format!("{g:.2} GB"))
                    .unwrap_or_else(|| "n/a".into());
                let cost = (m["hot_monthly_cost_cents"].as_u64().unwrap_or(0) as f64) / 100.0;
                let full_flag = if m["disk_full"].as_bool().unwrap_or(false) {
                    " [DISK FULL]"
                } else {
                    ""
                };
                lines.push(format!(
                    "  - {mid}: tier={tier} used={used} allocated={mgb} GB hot_monthly=${cost:.2}{full_flag}"
                ));
            }
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

pub(crate) fn parse_usd_to_cents(raw: &str) -> Result<u64, String> {
    let trimmed = raw.trim();
    let trimmed = trimmed.strip_prefix('$').unwrap_or(trimmed);
    if trimmed.is_empty() || trimmed.starts_with('-') || trimmed.starts_with('+') {
        return Err(format!("invalid USD amount: {raw:?}"));
    }
    let mut parts = trimmed.split('.');
    let whole_str = parts.next().unwrap_or("");
    let frac_str = parts.next();
    if parts.next().is_some()
        || whole_str.is_empty()
        || !whole_str.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(format!("invalid USD amount: {raw:?}"));
    }
    let whole: u64 = whole_str
        .parse()
        .map_err(|_| format!("invalid USD amount: {raw:?}"))?;
    let frac: u64 = match frac_str {
        None => 0,
        Some(f) if f.is_empty() || f.len() > 2 || !f.bytes().all(|b| b.is_ascii_digit()) => {
            return Err(format!("invalid USD amount: {raw:?}"));
        }
        Some(f) if f.len() == 1 => {
            f.parse::<u64>()
                .map_err(|_| format!("invalid USD amount: {raw:?}"))?
                * 10
        }
        Some(f) => f
            .parse::<u64>()
            .map_err(|_| format!("invalid USD amount: {raw:?}"))?,
    };
    let cents = whole
        .checked_mul(100)
        .and_then(|v| v.checked_add(frac))
        .ok_or_else(|| format!("USD amount overflow: {raw:?}"))?;
    if cents == 0 {
        return Err("USD amount must be greater than zero".into());
    }
    Ok(cents)
}

pub(crate) fn format_topup_human(val: &Value) -> String {
    val["url"].as_str().unwrap_or_default().to_owned()
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
    let is_topup = matches!(action, MeshCloudAction::Topup { .. });
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
        MeshCloudAction::Archive { machine } => http
            .post(format!(
                "{base}/api/mesh/cloud/{}/archive",
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
        MeshCloudAction::Delete {
            machine,
            confirm,
            recoverable,
            archive_id,
        } => {
            let mut body = serde_json::json!({
                "confirm": confirm,
                "mode": if recoverable { "recoverable" } else { "permanent" },
            });
            if let Some(arc) = archive_id {
                body["archive_id"] = serde_json::json!(arc);
            }
            http.post(format!(
                "{base}/api/mesh/cloud/{}/delete",
                urlencoding::encode(&machine)
            ))
            .header("Authorization", auth_header(token))
            .json(&body)
            .send()
        }
        MeshCloudAction::Recreate {
            machine,
            archive_id,
        } => {
            let mut body = serde_json::json!({});
            if let Some(arc) = archive_id {
                body["archive_id"] = serde_json::json!(arc);
            }
            http.post(format!(
                "{base}/api/mesh/cloud/{}/recreate",
                urlencoding::encode(&machine)
            ))
            .header("Authorization", auth_header(token))
            .json(&body)
            .send()
        }
        MeshCloudAction::ResizeVolume {
            machine,
            size_gb,
            confirm,
        } => http
            .post(format!(
                "{base}/api/mesh/cloud/{}/resize-volume",
                urlencoding::encode(&machine)
            ))
            .header("Authorization", auth_header(token))
            .json(&serde_json::json!({ "new_size_gb": size_gb, "confirm": confirm }))
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
        MeshCloudAction::Topup { pack, amount } => {
            let body = if let Some(pack_id) = pack {
                serde_json::json!({ "pack_id": pack_id })
            } else {
                let raw = amount.as_deref().unwrap_or_default();
                let amount_cents = match parse_usd_to_cents(raw) {
                    Ok(cents) => cents,
                    Err(error) => crate::cmd::print::fail(1, error),
                };
                serde_json::json!({ "pack_id": "custom", "amount_cents": amount_cents })
            };
            http.post(format!("{base}/api/mesh/cloud/checkout"))
                .header("Authorization", auth_header(token))
                .json(&body)
                .send()
        }
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
            } else if is_topup {
                println!("{}", format_topup_human(&val));
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
            let mut error = crate::cmd::print::CliError::http(status.as_u16(), &body);
            if let Some(message) = error
                .data
                .as_ref()
                .and_then(|value| value.get("error"))
                .filter(|error| error["code"] == "cloud_machines_unavailable")
                .and_then(|error| error["message"].as_str())
            {
                error.message = message.to_owned();
            }
            crate::cmd::print::fail_with(1, &error);
        }
        Err(error) => crate::cmd::print::fail(1, error.to_string()),
    }
}

#[cfg(test)]
#[path = "cloud_tests.rs"]
mod tests;
