use crate::cmd::mem::transport::mem_transport_error;
use crate::cmd::mem_secret_autounlock::{
    arm_unlock_request, json_summary, BackendDeclaration, SaveMasterPasswordOutcome,
};
use crate::cmd::print::print_human;
use crate::MemSecretAction;

// Secrets retain stable script exit codes over same-user local IPC:
// `3`=locked, `4`=logged-out, `5`=not found, `6`=backend unavailable.
pub fn handle_mem_secret(json_mode: bool, action: MemSecretAction) {
    use std::io::{Read, Write};

    let (base, _) = crate::read_api_config();
    let http = match crate::build_local_api() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: failed to build HTTP client: {}", e);
            std::process::exit(1);
        }
    };
    let base = base.as_str();

    let send_request = |req: crate::local_api::LocalApiRequest| -> (u16, serde_json::Value) {
        match req.send() {
            Ok(r) => {
                let status = r.status().as_u16();
                let body: serde_json::Value = r.json().unwrap_or(serde_json::Value::Null);
                (status, body)
            }
            Err(e) => (
                0,
                serde_json::json!({"error": {"kind": "backend", "message": mem_transport_error(base, &e)}}),
            ),
        }
    };

    let exit_for_status = |status: u16| -> Option<i32> {
        match status {
            200 | 201 | 204 => None,
            401 => Some(4), // LoggedOut
            423 => Some(3), // Locked
            404 => Some(5), // NotFound
            503 => Some(6), // Backend unavailable
            0 => Some(6),   // network failure
            _ => Some(1),
        }
    };

    let print_or_exit = |status: u16, body: serde_json::Value| -> ! {
        if let Some(code) = exit_for_status(status) {
            if json_mode {
                eprintln!(
                    "{}",
                    serde_json::to_string_pretty(&body).unwrap_or_default()
                );
            } else if let Some(err) = body.get("error") {
                let kind = err.get("kind").and_then(|k| k.as_str()).unwrap_or("error");
                let msg = err
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or(match kind {
                        "logged_out" => {
                            "secret store: logged out — run `codeterm mem secret unlock`"
                        }
                        "locked" => "secret store: vault locked — run `codeterm mem secret unlock`",
                        "not_found" => "secret store: not found",
                        "already_exists" => {
                            "secret store: already exists (use --overwrite to replace)"
                        }
                        _ => "secret store: error",
                    });
                eprintln!("Error: {}", msg);
            } else {
                eprintln!("Error: HTTP {}", status);
            }
            std::process::exit(code);
        }
        if json_mode {
            println!(
                "{}",
                serde_json::to_string_pretty(&body).unwrap_or_default()
            );
        } else {
            print_human(&body);
        }
        std::process::exit(0);
    };

    match action {
        MemSecretAction::Status => {
            let (status, body) = send_request(http.get(format!("{}/api/secrets/status", base)));
            print_or_exit(status, body);
        }
        MemSecretAction::Sync => {
            let (status, body) = send_request(http.post(format!("{}/api/secrets/sync", base)));
            print_or_exit(status, body);
        }
        MemSecretAction::Save {
            name,
            value,
            folder,
            org,
            collection,
            notes,
            overwrite,
        } => {
            let resolved_value = match value {
                Some(v) => v,
                None => {
                    let mut s = String::new();
                    if std::io::stdin().read_to_string(&mut s).is_err() || s.is_empty() {
                        eprintln!("Error: no value provided (pass --value or pipe to stdin)");
                        std::process::exit(2);
                    }
                    s.trim_end_matches('\n').to_string()
                }
            };
            let payload = serde_json::json!({
                "name": name,
                "value": resolved_value,
                "folder": folder,
                "organization": org,
                "collection": collection,
                "notes": notes,
                "overwrite": overwrite,
            });
            let (status, body) = send_request(
                http.post(format!("{}/api/secrets/items", base))
                    .json(&payload),
            );
            print_or_exit(status, body);
        }
        MemSecretAction::Get { name, json } => {
            let url = format!(
                "{}/api/secrets/items/{}/value",
                base,
                urlencoding::encode(&name)
            );
            let (status, body) = send_request(http.post(url));
            if exit_for_status(status).is_some() {
                print_or_exit(status, body);
            }
            if json || json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&body).unwrap_or_default()
                );
            } else {
                let value = body
                    .get("value")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let _ = std::io::stdout().write_all(value.as_bytes());
            }
        }
        MemSecretAction::Search { query } => {
            let url = format!(
                "{}/api/secrets/items?q={}",
                base,
                urlencoding::encode(&query)
            );
            let (status, body) = send_request(http.get(url));
            print_or_exit(status, body);
        }
        MemSecretAction::List { org, folder, limit } => {
            let mut url = format!("{}/api/secrets/items", base);
            let mut sep = '?';
            if let Some(o) = &org {
                url.push_str(&format!("{}organization={}", sep, urlencoding::encode(o)));
                sep = '&';
            }
            if let Some(f) = &folder {
                url.push_str(&format!("{}folder={}", sep, urlencoding::encode(f)));
                sep = '&';
            }
            if let Some(l) = limit {
                url.push_str(&format!("{}limit={}", sep, l));
            }
            let (status, body) = send_request(http.get(url));
            print_or_exit(status, body);
        }
        MemSecretAction::Delete { name, yes } => {
            if !yes {
                use std::io::IsTerminal;
                if !std::io::stdin().is_terminal() {
                    eprintln!("Error: refusing to prompt on non-interactive stdin — pass --yes to delete '{}'", name);
                    std::process::exit(1);
                }
                eprint!("Delete secret '{}'? [y/N] ", name);
                let _ = std::io::stderr().flush();
                let mut answer = String::new();
                if std::io::stdin().read_line(&mut answer).is_err() {
                    eprintln!("Aborted.");
                    std::process::exit(1);
                }
                if !answer.trim().eq_ignore_ascii_case("y") {
                    eprintln!("Aborted.");
                    std::process::exit(1);
                }
            }
            let url = format!("{}/api/secrets/items/{}", base, urlencoding::encode(&name));
            let (status, body) = send_request(http.delete(url));
            print_or_exit(status, body);
        }
        MemSecretAction::Unlock { email, password } => {
            // Master password resolution order: 1. --password flag (worst — visible to other users via `ps`) 2. stdin (preferred for scripted use) 3.
            let resolved: Option<String> = match password {
                Some(p) => Some(p),
                None => {
                    use std::io::IsTerminal;
                    if std::io::stdin().is_terminal() {
                        // Interactive shell — don't block waiting for stdin bytes that the user never piped. Fall through to server-side auto-unlock attempt.
                        None
                    } else {
                        let mut s = String::new();
                        if std::io::stdin().read_to_string(&mut s).is_err() || s.trim().is_empty() {
                            None
                        } else {
                            Some(s.trim_end_matches('\n').to_string())
                        }
                    }
                }
            };
            if let Some(pwd) = resolved {
                let payload = serde_json::json!({
                    "master_password": pwd,
                    "email": email,
                });
                let (status, body) = send_request(
                    http.post(format!("{}/api/secrets/unlock", base))
                        .json(&payload),
                );
                print_or_exit(status, body);
            } else {
                // Auto-unlock with stored creds. Daemon returns ok=false if nothing's stored — surface that as a clear error.
                let (status, body) = send_request(
                    http.post(format!("{}/api/secrets/auto-unlock", base))
                        .json(&serde_json::json!({})),
                );
                if status == 200 && body.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                    print_or_exit(status, body);
                } else if status == 200 {
                    eprintln!(
                        "Error: no stored master password — pass --password or pipe to stdin"
                    );
                    std::process::exit(2);
                } else {
                    print_or_exit(status, body);
                }
            }
        }
        MemSecretAction::Lock => {
            let (status, body) = send_request(
                http.post(format!("{}/api/secrets/lock", base))
                    .json(&serde_json::json!({})),
            );
            print_or_exit(status, body);
        }
        MemSecretAction::Logout => {
            let (status, body) = send_request(
                http.post(format!("{}/api/secrets/logout", base))
                    .json(&serde_json::json!({})),
            );
            print_or_exit(status, body);
        }
        MemSecretAction::SaveMasterPassword { password } => {
            let pw = match password {
                Some(p) => p,
                None => read_stdin().trim().to_string(),
            };
            if pw.is_empty() {
                eprintln!("Error: master password is empty");
                std::process::exit(1);
            }

            let (decl_status, decl_body) =
                send_request(http.get(format!("{}/api/secrets/status", base)));
            if exit_for_status(decl_status).is_some() {
                print_or_exit(decl_status, decl_body);
            }
            let declaration = BackendDeclaration::from_status(&decl_body);

            // The credential leaves this process only when the active backend
            // declares it can use one; otherwise nothing is sent or stored.
            let unlock_status = if declaration.auto_unlock_capable {
                let (status, _) = send_request(
                    http.post(format!("{}/api/secrets/unlock", base))
                        .json(&arm_unlock_request(&pw)),
                );
                Some(status)
            } else {
                None
            };
            // Whether auto-unlock is armed is the backend's answer, re-read from
            // the status surface — never this command's assumption.
            let vault_accepted = matches!(unlock_status, Some(s) if exit_for_status(s).is_none());
            let armed_after = vault_accepted && {
                let (_, after) = send_request(http.get(format!("{}/api/secrets/status", base)));
                after
                    .get("master_password_saved")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            };

            let outcome =
                SaveMasterPasswordOutcome::decide(&declaration, unlock_status, armed_after);
            if json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json_summary(&declaration, &outcome))
                        .unwrap_or_default()
                );
            } else {
                for line in outcome.lines() {
                    if outcome.is_success() {
                        println!("{}", line);
                    } else {
                        eprintln!("{}", line);
                    }
                }
            }
            std::process::exit(outcome.exit_code());
        }
    }
}

fn read_stdin() -> String {
    use std::io::Read;
    let mut buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut buf);
    buf
}
