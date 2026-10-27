use clap::Subcommand;
#[derive(Subcommand)]
pub enum AccountAction {
    /// Sign in using the system browser and wait for completion
    Login,
    /// Show the current plan and stale-cache marker
    Plan,
    /// Open an upgrade checkout in the system browser
    Upgrade { plan_key: String },
    /// Open subscription management in the system browser
    Portal,
    /// Show the signed-in identity
    Status,
    /// Sign out and delete the saved credential
    Logout,
    /// Manage the encrypted Neighbors channel on this machine
    Neighbors {
        #[command(subcommand)]
        action: NeighborAction,
    },
}

#[derive(Subcommand)]
pub enum NeighborAction {
    /// List friendships, local scopes and pending incoming/outgoing requests
    List,
    /// Show local channel enrollment and gateway bindings
    Status,
    /// Receive Neighbors on this machine; other machines stop receiving
    Enroll {
        #[arg(long, required = true)]
        confirm_takeover: bool,
    },
    /// Rotate this installation's keys and revoke the previous pair
    Rotate,
    /// Revoke this machine's Neighbor keys
    Revoke,
    /// List active own key IDs
    OwnKeys,
    /// Bind a friendship to an independent local gateway agent tab
    Bind {
        friendship_id: String,
        #[arg(long)]
        tab: String,
    },
    /// Read a Neighbor's local permission policy
    Permission { friendship_id: String },
    /// General Agent only: grant a bounded scope authored in its pad
    Grant {
        friendship_id: String,
        #[arg(long)]
        from_pad: String,
        #[arg(long)]
        allow_tab_requests: bool,
    },
    /// General Agent only: deny this Neighbor's requests
    Deny { friendship_id: String },
    /// Encrypt and send one authored message; prints only its ID and status
    Send {
        friendship_id: String,
        #[arg(long)]
        from_pad: String,
        #[arg(long, default_value = "question", value_parser = ["question", "answer", "refusal", "tab_request"])]
        kind: String,
        #[arg(long)]
        in_reply_to: Option<String>,
    },
    /// Read pending, delivered or not_delivered from the relay receipt
    DeliveryStatus { message_id: String },
}
