use crate::UsageAction;

pub fn handle_usage(
    json_mode: bool,
    action: UsageAction,
    http: &crate::local_api::LocalApiClient,
    base: &str,
    token: Option<&str>,
) {
    match action {
        UsageAction::Latest {
            provider,
            account_id,
        } => {
            let url = build_url(base, "/api/usage/latest", &provider, account_id.as_deref());
            get_json(http, &url, token, json_mode, render_latest);
        }
        UsageAction::Projection {
            provider,
            account_id,
        } => {
            let url = build_url(
                base,
                "/api/usage/projection",
                &provider,
                account_id.as_deref(),
            );
            get_json(http, &url, token, json_mode, render_projection);
        }
        UsageAction::Refresh {
            provider,
            account_id,
        } => {
            let mut body = serde_json::json!({ "provider": provider });
            if let Some(a) = account_id {
                body["account_id"] = serde_json::json!(a);
            }
            let mut req = http.post(format!("{}/api/usage/refresh", base)).json(&body);
            if let Some(t) = token {
                req = req.header("Authorization", format!("Bearer {}", t));
            }
            match req
                .send()
                .and_then(|r| r.error_for_status())
                .and_then(|r| r.json::<serde_json::Value>())
            {
                Ok(v) => {
                    if json_mode {
                        println!("{}", serde_json::to_string(&v).unwrap_or_default());
                    } else {
                        println!("queued: refresh probe for {}", provider);
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}

fn build_url(base: &str, path: &str, provider: &str, account_id: Option<&str>) -> String {
    let mut url = format!(
        "{}{}?provider={}",
        base,
        path,
        urlencoding::encode(provider)
    );
    if let Some(a) = account_id {
        url.push_str(&format!("&account_id={}", urlencoding::encode(a)));
    }
    url
}

fn get_json<F: FnOnce(&serde_json::Value)>(
    http: &crate::local_api::LocalApiClient,
    url: &str,
    token: Option<&str>,
    json_mode: bool,
    render: F,
) {
    let mut req = http.get(url);
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {}", t));
    }
    let resp = match req.send().and_then(|r| r.error_for_status()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };
    let v: serde_json::Value = match resp.json() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: parse json: {}", e);
            std::process::exit(1);
        }
    };
    if json_mode {
        println!("{}", serde_json::to_string(&v).unwrap_or_default());
    } else {
        render(&v);
    }
}

fn render_latest(v: &serde_json::Value) {
    if v.is_null() {
        println!("(no snapshot)");
        return;
    }
    let sess = v.get("session_pct").and_then(|x| x.as_f64());
    let week = v.get("weekly_pct").and_then(|x| x.as_f64());
    let sess_reset = v.get("session_resets_at").and_then(|x| x.as_i64());
    let week_reset = v.get("weekly_resets_at").and_then(|x| x.as_i64());
    println!("session: {}  reset {}", fmt_pct(sess), fmt_ts(sess_reset));
    println!("weekly:  {}  reset {}", fmt_pct(week), fmt_ts(week_reset));
}

fn render_projection(v: &serde_json::Value) {
    if v.is_null() {
        println!("(no projection — not enough data)");
        return;
    }
    if let Some(s) = v.get("session").filter(|s| !s.is_null()) {
        let proj = s
            .get("projected_pct")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0);
        let rate = s
            .get("rate_per_min")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0);
        let over = s
            .get("will_exceed")
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        println!(
            "session: projected {:.0}% by reset · {:.2} %/min{}",
            proj,
            rate,
            if over { " · WILL EXCEED" } else { "" }
        );
    } else {
        println!("session: (no projection)");
    }
    if let Some(w) = v.get("weekly").filter(|w| !w.is_null()) {
        let eod = w
            .get("end_of_today_pct")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0);
        let eoc = w
            .get("end_of_cycle_pct")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0);
        let wd = w
            .get("avg_weekday_spend")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0);
        let we = w
            .get("avg_weekend_spend")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0);
        let rest = w
            .get("rest_of_today_est")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0);
        let conf = w.get("confidence").and_then(|x| x.as_str()).unwrap_or("?");
        println!(
            "weekly:  end-of-today {:.0}% · end-of-cycle {:.0}% [{}]",
            eod, eoc, conf
        );
        println!(
            "         rest-of-today ~{:.1}% · per weekday ~{:.1}% · per weekend ~{:.1}%",
            rest, wd, we
        );
    } else {
        println!("weekly:  (no projection)");
    }
}

fn fmt_pct(v: Option<f64>) -> String {
    match v {
        Some(x) => format!("{:.0}%", x),
        None => "—".to_string(),
    }
}

fn fmt_ts(ts: Option<i64>) -> String {
    match ts {
        Some(t) if t > 0 => {
            let secs = t / 1000;
            let days = secs / 86_400;
            let sec_of_day = secs.rem_euclid(86_400);
            let hh = sec_of_day / 3600;
            let mm = (sec_of_day % 3600) / 60;
            let z = days + 719_468;
            let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
            let doe = z - era * 146_097;
            let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
            let y = yoe + era * 400;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let d = doy - (153 * mp + 2) / 5 + 1;
            let m = if mp < 10 { mp + 3 } else { mp - 9 };
            let y = if m <= 2 { y + 1 } else { y };
            format!("{:04}-{:02}-{:02} {:02}:{:02}", y, m, d, hh, mm)
        }
        _ => "—".to_string(),
    }
}
