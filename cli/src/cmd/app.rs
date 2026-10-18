use crate::args::app::AppAction;
use codeterm_process_identity::PinnedProcess;
use codeterm_types::app_quit::{
    QuitApiReply, QuitCompletion, QuitOutput, QuitReceipt, QuitRefusal, QuitSnapshot,
    QuitUncertainty, QUIT_FAILURE_DEADLINE,
};
use reqwest::blocking::{Client, Response};
use serde::de::DeserializeOwned;
use std::{
    io::{self, Read},
    net::IpAddr,
    time::{Duration, Instant},
};

pub fn handle(action: &AppAction, json: bool, remote: bool) -> ! {
    let output = match action {
        AppAction::Quit { exact: true } if !remote => run(),
        _ => QuitOutput::Refused {
            reason: QuitRefusal::IdentityMismatch,
        },
    };
    crate::cmd::print::finish_app_quit(output, json)
}

fn run() -> QuitOutput {
    let deadline = Instant::now() + QUIT_FAILURE_DEADLINE;
    let config = crate::cmd::identify::resolve_api_config();
    let host = config.host.trim_matches(['[', ']']).parse::<IpAddr>();
    let Ok(host) = host else {
        return refused(QuitRefusal::IdentityMismatch);
    };
    if !host.is_loopback() || !(1..=65535).contains(&config.port) {
        return refused(QuitRefusal::IdentityMismatch);
    }
    let Some(token) = config.token.filter(|token| !token.is_empty()) else {
        return refused(QuitRefusal::NoInstance);
    };
    let base = format!(
        "http://{}",
        std::net::SocketAddr::new(host, config.port as u16)
    );
    let client = match Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(QUIT_FAILURE_DEADLINE)
        .build()
    {
        Ok(client) => client,
        Err(_) => return refused(QuitRefusal::Unavailable),
    };
    let response = match client
        .get(format!("{base}/api/app/quit/snapshot"))
        .bearer_auth(&token)
        .timeout(remaining(deadline))
        .send()
    {
        Ok(response) => response,
        Err(_) => return refused(QuitRefusal::NoInstance),
    };
    if !response.status().is_success() {
        return refused(QuitRefusal::Unavailable);
    }
    let snapshot: QuitSnapshot = match decode(response) {
        Ok(snapshot) => snapshot,
        Err(_) => return refused(QuitRefusal::IdentityMismatch),
    };
    let paths = match codeterm_paths::instance_identity::ResolvedInstancePaths::resolve(
        &config.data_dir,
        &config.data_dir.join("config.toml"),
    ) {
        Ok(paths) => paths,
        Err(_) => return refused(QuitRefusal::IdentityMismatch),
    };
    if !snapshot.valid()
        || snapshot.ready.identity.data != paths.data()
        || snapshot.ready.identity.config != paths.config()
        || snapshot.ready.api_address.port() != config.port as u16
        || !(snapshot.ready.api_address.ip().is_loopback()
            || snapshot.ready.api_address.ip().is_unspecified())
    {
        return refused(QuitRefusal::IdentityMismatch);
    }
    let server = match pin(&snapshot.ready.process) {
        Ok(pin) => pin,
        Err(_) => return refused(QuitRefusal::IdentityMismatch),
    };
    let native = match snapshot
        .client
        .as_ref()
        .map(|client| pin(&client.process))
        .transpose()
    {
        Ok(pin) => pin,
        Err(_) => return refused(QuitRefusal::IdentityMismatch),
    };
    if remaining(deadline).is_zero() {
        return refused(QuitRefusal::Unavailable);
    }
    let response = match client
        .post(format!("{base}/api/app/quit"))
        .bearer_auth(&token)
        .timeout(remaining(deadline))
        .json(&snapshot)
        .send()
    {
        Ok(response) => response,
        Err(_) => return uncertain(QuitUncertainty::Disconnected),
    };
    let successful_http = response.status().is_success();
    let reply: QuitApiReply = match decode(response) {
        Ok(reply) => reply,
        Err(_) => return uncertain(QuitUncertainty::InvalidCompletion),
    };
    if !successful_http && matches!(&reply, QuitApiReply::Completed { .. }) {
        return uncertain(QuitUncertainty::InvalidCompletion);
    }
    let completion = match validate_completion(&snapshot, reply) {
        Ok(completion) => completion,
        Err(output) => return output,
    };
    if server.wait_exit(remaining(deadline)).is_err()
        || native
            .as_ref()
            .is_some_and(|native| native.wait_exit(remaining(deadline)).is_err())
    {
        return uncertain(QuitUncertainty::ExitUnobserved);
    }
    QuitOutput::Confirmed {
        receipt: Box::new(QuitReceipt::after_observed_exits(snapshot, completion)),
    }
}

fn pin(
    expected: &codeterm_types::server_bootstrap::ProcessProvenance,
) -> io::Result<PinnedProcess> {
    let pin = PinnedProcess::open(expected.pid)?;
    if pin.provenance()? != *expected {
        return Err(io::Error::other("Process incarnation differs"));
    }
    Ok(pin)
}
fn remaining(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}
fn decode<T: DeserializeOwned>(response: Response) -> io::Result<T> {
    let mut bytes = Vec::new();
    response.take(32769).read_to_end(&mut bytes)?;
    if bytes.len() > 32768 {
        return Err(io::Error::other("Quit response exceeds bound"));
    }
    serde_json::from_slice(&bytes).map_err(|_| io::Error::other("Invalid Quit response"))
}
fn refused(reason: QuitRefusal) -> QuitOutput {
    QuitOutput::Refused { reason }
}
fn uncertain(reason: QuitUncertainty) -> QuitOutput {
    QuitOutput::Uncertain { reason }
}
fn validate_completion(
    expected: &QuitSnapshot,
    reply: QuitApiReply,
) -> Result<QuitCompletion, QuitOutput> {
    match reply {
        QuitApiReply::Completed {
            expected: actual,
            shutdown_completion,
        } if expected.matches(&actual) && shutdown_completion == QuitCompletion::default() => {
            Ok(shutdown_completion)
        }
        QuitApiReply::Completed { .. } => Err(uncertain(QuitUncertainty::InvalidCompletion)),
        QuitApiReply::Refused { reason } => Err(refused(reason)),
        QuitApiReply::Uncertain { reason } => Err(uncertain(reason)),
    }
}
#[cfg(test)]
mod tests;
