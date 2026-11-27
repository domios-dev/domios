use clap::{Subcommand, ValueEnum};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum SecretInputKindArg {
    Code,
    Token,
    Password,
    Key,
}

impl SecretInputKindArg {
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Token => "token",
            Self::Password => "password",
            Self::Key => "key",
        }
    }
}

/// Request owner-submitted masked input for a local or Full-access mesh machine.
/// Never accepts secret values on the command line or stdin; the owner submits them in UI.
#[derive(Subcommand)]
pub enum SecretAction {
    /// Create a masked owner input request for a closed consumer (never accepts a value)
    Request {
        /// Target machine name or installation id (defaults to local machine)
        #[arg(long)]
        machine: Option<String>,
        /// Explicit consumer: vault:<NAME>, provider-login:<tab>, or provider-key:<provider>
        #[arg(long)]
        target: String,
        /// Kind of secret requested from the owner
        #[arg(long, value_enum)]
        kind: SecretInputKindArg,
        /// Short human-readable reason shown on the owner's masked card
        #[arg(long)]
        reason: String,
        /// Allow overwriting an existing vault slot (vault:<NAME> only)
        #[arg(long)]
        overwrite: bool,
        /// Optional provider id binding (provider-login:<tab>)
        #[arg(long)]
        provider: Option<String>,
        /// Optional active login flow id (provider-login:<tab>)
        #[arg(long)]
        login_id: Option<String>,
        /// Optional agent session id binding (provider-login:<tab>)
        #[arg(long)]
        session_id: Option<String>,
        /// Optional runtime binding nonce (provider-login:<tab>)
        #[arg(long)]
        nonce: Option<String>,
        /// Optional runtime epoch binding (provider-login:<tab>)
        #[arg(long)]
        epoch: Option<u64>,
        /// Optional target tab id (provider-key:<provider>)
        #[arg(long)]
        tab: Option<String>,
        /// Optional declared login method id (provider-login or provider-key)
        #[arg(long)]
        method: Option<String>,
        /// Request lifetime in seconds (30..=900, default 300)
        #[arg(long)]
        ttl_secs: Option<u64>,
    },
    /// Inspect typed status of a secure input request (never returns the secret value)
    Status {
        /// Request id returned by `domios secret request`
        request_id: String,
    },
    /// Cancel a pending secure input request
    Cancel {
        /// Request id returned by `domios secret request`
        request_id: String,
        /// Optional cancellation reason
        #[arg(long)]
        reason: Option<String>,
    },
}
