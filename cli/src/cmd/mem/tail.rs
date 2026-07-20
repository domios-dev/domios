use crate::client::SocketClient;

fn fetch_tail(
    client: &mut SocketClient,
    since: Option<&str>,
    limit: usize,
) -> Result<Vec<serde_json::Value>, String> {
    let val = client.call(
        "mem.tail",
        Some(serde_json::json!({ "since": since, "limit": limit })),
    )?;
    Ok(val
        .get("rows")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default())
}

pub(crate) fn run_mem_tail(
    client: &mut SocketClient,
    json_mode: bool,
    follow: bool,
    limit: usize,
) {
    let print_row = |row: &serde_json::Value| {
        let ts = row
            .get("updated_at")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .split('.')
            .next()
            .unwrap_or("?")
            .replace('T', " ");
        let kind = row.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
        let who = row
            .get("changed_by")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let title = row.get("title").and_then(|v| v.as_str()).unwrap_or("?");
        let area = row.get("area").and_then(|v| v.as_str()).unwrap_or("(none)");
        println!(
            "{} | {:<12} | {:<15} | {:<60} | {}",
            ts,
            kind,
            who,
            title.chars().take(60).collect::<String>(),
            area
        );
    };

    match fetch_tail(client, None, limit) {
        Ok(rows) => {
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&rows).unwrap());
            } else if rows.is_empty() {
                println!("(no recent writes)");
            } else {
                for row in rows.iter().rev() {
                    print_row(row);
                }
            }
            if !follow {
                return;
            }
            let mut last_seen: Option<String> = rows
                .iter()
                .filter_map(|r| {
                    r.get("updated_at")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                })
                .max();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let next = match fetch_tail(client, last_seen.as_deref(), limit) {
                    Ok(rows) => rows,
                    Err(e) => {
                        eprintln!("tail fetch: {}", e);
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        continue;
                    }
                };
                if !next.is_empty() {
                    for row in next.iter().rev() {
                        print_row(row);
                    }
                    if let Some(newest) = next
                        .iter()
                        .filter_map(|r| {
                            r.get("updated_at")
                                .and_then(|v| v.as_str())
                                .map(String::from)
                        })
                        .max()
                    {
                        last_seen = Some(newest);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
