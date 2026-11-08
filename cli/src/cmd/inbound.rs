//! Messages waiting for this tab, carried in what the agent reads next: a host tool result, or the
//! output of a `domios` command it ran in its own terminal.

use serde_json::json;
use std::io::{ErrorKind, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};

/// Messages the daemon took off this tab's queue for one piece of output.
pub(crate) struct Claim {
    pub(crate) id: String,
    pub(crate) text: String,
}

/// The daemon counts a claim delivered only on `settle(id, true)`, sent after the output holding it
/// was written. This process is the claim's only writer, so `false` proves no byte of it went out.
pub(crate) trait Inbound {
    fn claim(&mut self) -> Option<Claim>;
    fn settle(&mut self, id: &str, written: bool);
}

#[cfg(test)]
pub(crate) struct NoInbound;

#[cfg(test)]
impl Inbound for NoInbound {
    fn claim(&mut self) -> Option<Claim> {
        None
    }
    fn settle(&mut self, _id: &str, _written: bool) {}
}

pub(crate) struct Daemon {
    tab: String,
    nonce: Option<String>,
}

impl Daemon {
    pub(crate) fn new(tab: &str, nonce: Option<&str>) -> Self {
        Self {
            tab: tab.into(),
            nonce: nonce.map(str::to_owned),
        }
    }

    fn call(
        &self,
        method: &str,
        mut params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        params["caller_tab_id"] = json!(self.tab);
        // Without it the tab credential supplies the binding, as for a launch-file credential.
        if let Some(nonce) = &self.nonce {
            params["session_binding_nonce"] = json!(nonce);
        }
        let (mut client, _) = crate::client::SocketClient::connect_local()?;
        client.call(method, Some(params))
    }
}

impl Inbound for Daemon {
    fn claim(&mut self) -> Option<Claim> {
        let id = uuid::Uuid::new_v4().to_string();
        match self.call("agent.inbound_claim", json!({ "claim_id": id })) {
            Ok(reply) => Some(Claim {
                text: reply["text"].as_str()?.to_owned(),
                id,
            }),
            // The daemon may have claimed for a reply that never arrived; nothing here wrote it.
            Err(_) => {
                self.settle(&id, false);
                None
            }
        }
    }

    fn settle(&mut self, id: &str, written: bool) {
        // A claim left unsettled stays uncertain in the daemon and is never replayed.
        if let Err(reason) = self.call(
            "agent.inbound_settle",
            json!({ "claim_id": id, "written": written }),
        ) {
            if written {
                eprintln!("domios: inbound delivery unconfirmed: {reason}");
            }
        }
    }
}

/// Where this command's output goes and what it could carry.
struct Surface {
    json: bool,
    stdout_terminal: bool,
    stdin_terminal: bool,
    daemon_reached: bool,
    tab_identity: bool,
}

/// A terminal on stdout means no pipe, capture or file sits between this command and whoever reads
/// the terminal, so `domios x | grep`, `$(domios pad path)` and `> file` never carry messages. A
/// terminal on stdin rules out pipelines feeding this command and scripts reading a file.
fn carries_messages(surface: &Surface) -> bool {
    !surface.json
        && surface.stdout_terminal
        && surface.stdin_terminal
        && surface.daemon_reached
        && surface.tab_identity
}

static DAEMON_REACHED: AtomicBool = AtomicBool::new(false);

/// A local command must not wait on a daemon it never needed.
pub(crate) fn mark_daemon_reached() {
    DAEMON_REACHED.store(true, Ordering::Relaxed);
}

/// Runs after a command finished normally. A command that exits with a failure carries nothing.
pub(crate) fn after_command() {
    let tab = crate::nonempty_env("CODETERM_TAB_ID");
    let surface = Surface {
        json: crate::cmd::error_contract::json_mode(),
        stdout_terminal: std::io::stdout().is_terminal(),
        stdin_terminal: std::io::stdin().is_terminal(),
        daemon_reached: DAEMON_REACHED.load(Ordering::Relaxed),
        tab_identity: tab.is_some(),
    };
    let Some(tab) = tab.filter(|_| carries_messages(&surface)) else {
        return;
    };
    let nonce = crate::nonempty_env("CODETERM_SESSION_BINDING_NONCE");
    append_messages(
        &mut std::io::stdout(),
        &mut Daemon::new(&tab, nonce.as_deref()),
    );
}

enum Written {
    All,
    Nothing,
    Uncertain,
}

fn write_counted(out: &mut impl Write, bytes: &[u8]) -> Written {
    let mut sent = 0;
    while sent < bytes.len() {
        match out.write(&bytes[sent..]) {
            Ok(0) => break,
            Ok(count) => sent += count,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    if sent == 0 {
        Written::Nothing
    } else if sent < bytes.len() || out.flush().is_err() {
        Written::Uncertain
    } else {
        Written::All
    }
}

fn append_messages(out: &mut impl Write, inbound: &mut impl Inbound) {
    // A terminal that cannot take this command's own output cannot take a claim either.
    if out.flush().is_err() {
        return;
    }
    let Some(claim) = inbound.claim() else {
        return;
    };
    match write_counted(out, format!("\n{}", claim.text).as_bytes()) {
        Written::All => inbound.settle(&claim.id, true),
        Written::Nothing => inbound.settle(&claim.id, false),
        // Part of it reached the reader, so returning it could deliver it twice.
        Written::Uncertain => {}
    }
}

#[cfg(test)]
mod tests;
