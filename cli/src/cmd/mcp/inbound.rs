use serde_json::json;

/// Messages the daemon took off this tab's queue for one tool result.
pub(super) struct Claim {
    pub(super) id: String,
    pub(super) text: String,
}

/// The daemon counts a claim delivered only on `settle(id, true)`, sent after the tool result holding
/// it was written. This process is the claim's only writer, so `false` proves it was never written.
pub(super) trait Inbound {
    fn claim(&mut self) -> Option<Claim>;
    fn settle(&mut self, id: &str, written: bool);
}

#[cfg(test)]
pub(super) struct NoInbound;

#[cfg(test)]
impl Inbound for NoInbound {
    fn claim(&mut self) -> Option<Claim> {
        None
    }
    fn settle(&mut self, _id: &str, _written: bool) {}
}

pub(super) struct Daemon {
    tab: String,
    nonce: String,
    process: u32,
    sequence: u64,
}

impl Daemon {
    pub(super) fn new(tab: &str, nonce: &str) -> Self {
        Self {
            tab: tab.into(),
            nonce: nonce.into(),
            process: std::process::id(),
            sequence: 0,
        }
    }

    fn call(
        &self,
        method: &str,
        mut params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        params["caller_tab_id"] = json!(self.tab);
        params["session_binding_nonce"] = json!(self.nonce);
        let (mut client, _) = crate::client::SocketClient::connect_local()?;
        client.call(method, Some(params))
    }
}

impl Inbound for Daemon {
    fn claim(&mut self) -> Option<Claim> {
        self.sequence += 1;
        let id = format!("mcp-{}-{}", self.process, self.sequence);
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
            eprintln!("mcp_serve_inbound_settle_failed: {reason}");
        }
    }
}
