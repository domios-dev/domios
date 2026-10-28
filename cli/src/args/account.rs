use clap::Subcommand;
#[derive(Subcommand)]
pub enum AccountAction {
    /// Sign in and wait for completion
    Login {
        /// Print the authorization URL and accept the whole redirected callback URL
        #[arg(long)]
        no_browser: bool,
        /// Complete the pending browserless sign-in using a whole callback URL from this tab's pad
        #[arg(long, requires = "no_browser")]
        code_from_pad: Option<String>,
    },
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
    /// Locally flag a suspicious incoming message for the owner without replying
    Flag {
        message_id: String,
        #[arg(long)]
        friendship: String,
        #[arg(long, value_parser = ["suspicious_instructions", "private_data_request", "malicious_link", "spam", "other"])]
        category: String,
        /// Short explanation without message text or secrets (at most 280 characters)
        #[arg(long)]
        reason: String,
    },
    /// List friendships, local scopes and pending incoming/outgoing requests
    List,
    /// Set this friend's incoming request cap per hour (owner or General)
    SetLimit {
        friendship_id: String,
        #[arg(long)]
        per_hour: u16,
    },
    /// Set daily incoming messages; null removes the per-friend daily cap
    SetDailyLimit {
        friendship_id: String,
        #[arg(long)]
        daily_limit: NeighborLimit,
    },
    /// Set daily incoming file bytes; null restores the 50 MiB default
    SetFileDailyLimit {
        friendship_id: String,
        #[arg(long)]
        file_daily_limit_bytes: NeighborLimit,
    },
    /// Show local channel enrollment and gateway bindings
    Status,
    /// Receive Neighbors on this machine; other machines stop receiving
    Enroll {
        #[arg(long, required = true)]
        confirm_takeover: bool,
        /// Receiver revision shown by status; refresh and confirm again if stale
        #[arg(long)]
        expected_receiver_revision: u64,
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
    /// General Agent only: apply permission edits and restart the concierge fresh
    Grant {
        friendship_id: String,
        #[command(flatten)]
        fields: NeighborGrant,
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

#[derive(Clone, Debug)]
pub enum NeighborLimit {
    Default,
    Value(u64),
}

impl std::str::FromStr for NeighborLimit {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value == "null" {
            Ok(Self::Default)
        } else {
            value
                .parse()
                .map(Self::Value)
                .map_err(|_| "Expected a non-negative integer or null".into())
        }
    }
}

#[derive(clap::Args, Default)]
pub struct NeighborGrant {
    /// Scope text in this tab's pad; omitted keeps the current scope
    #[arg(long)]
    pub from_pad: Option<String>,
    #[arg(long, action = clap::ArgAction::Set)]
    pub allowed: Option<bool>,
    #[arg(long, action = clap::ArgAction::Set)]
    pub allow_memory: Option<bool>,
    /// all or comma-separated taxonomy area names
    #[arg(long, value_parser = memory_selection)]
    pub memory_areas: Option<NeighborSelection>,
    #[arg(long, action = clap::ArgAction::Set)]
    pub allow_files: Option<bool>,
    /// Absolute directory, or default to clear the folder override
    #[arg(long)]
    pub folder: Option<String>,
    #[arg(long, action = clap::ArgAction::Set)]
    pub allow_tab_requests: Option<bool>,
    #[arg(long, action = clap::ArgAction::Set)]
    pub allow_interactive_content: Option<bool>,
    /// none or comma-separated local workspace IDs
    #[arg(long, value_parser = workspace_selection)]
    pub workspaces: Option<NeighborSelection>,
    #[arg(long, value_parser = ["none", "read_only", "folder", "full"])]
    pub machine_access: Option<String>,
}

#[derive(Clone, Debug)]
pub struct NeighborSelection(pub Option<Vec<String>>);

impl std::str::FromStr for NeighborSelection {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let items: Vec<String> = value
            .split(',')
            .map(|item| item.trim().to_owned())
            .collect();
        if items
            .iter()
            .any(|item| item.is_empty() || matches!(item.as_str(), "all" | "none"))
        {
            return Err("Expected non-empty comma-separated names without all or none".into());
        }
        Ok(Self(Some(items)))
    }
}

fn memory_selection(value: &str) -> Result<NeighborSelection, String> {
    if value == "all" {
        Ok(NeighborSelection(None))
    } else if value == "none" {
        Ok(NeighborSelection(Some(Vec::new())))
    } else {
        value.parse()
    }
}

fn workspace_selection(value: &str) -> Result<NeighborSelection, String> {
    if value == "none" {
        Ok(NeighborSelection(None))
    } else {
        value.parse()
    }
}
