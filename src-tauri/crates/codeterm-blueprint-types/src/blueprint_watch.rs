use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
pub const MIN_INTERVAL_MS: u64 = 10000;
pub const MAX_BUCKETS: usize = 16;
pub const COMMAND_RUNTIME_MS: u64 = 10000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandParameters {
    pub command: String,
    pub label: String,
    pub timeout_ms: u64,
}

impl CommandParameters {
    pub fn validate(&self) -> Result<(), String> {
        if self.command.trim().is_empty()
            || self.command.len() > 8192
            || self.command.contains('\0')
        {
            return Err("command requires nonempty text of at most 8192 bytes without NUL".into());
        }
        if self.label.trim().is_empty()
            || self.label.len() > 256
            || self.label.chars().any(char::is_control)
        {
            return Err("command label requires nonempty display text of at most 256 bytes".into());
        }
        if self.timeout_ms == 0 || self.timeout_ms > COMMAND_RUNTIME_MS {
            return Err("command timeout_ms must be positive and at most 10 seconds".into());
        }
        Ok(())
    }
}

/// Prompt class a respond-prompt handler requests an answer for; the answer path alone
/// chooses among the provider-declared options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SafeKind {
    DirectoryTrust,
    Onboarding,
    Update,
    Permission,
    Structured,
    Other(String),
}

impl SafeKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::DirectoryTrust => "directory_trust",
            Self::Onboarding => "onboarding",
            Self::Update => "update",
            Self::Permission => "permission",
            Self::Structured => "structured",
            Self::Other(kind) => kind,
        }
    }
}

impl Serialize for SafeKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SafeKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let kind = String::deserialize(deserializer)?;
        Ok(match kind.as_str() {
            "directory_trust" => Self::DirectoryTrust,
            "onboarding" => Self::Onboarding,
            "update" => Self::Update,
            "permission" => Self::Permission,
            "structured" => Self::Structured,
            _ => Self::Other(kind),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscalateTarget {
    Creator,
    Agent(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "target",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Route {
    Notify,
    Send(String),
    Cron(String),
    #[serde(rename = "blueprint", alias = "sentinel")]
    Sentinel(String),
    Stop,
    Command(CommandParameters),
    RespondPrompt(SafeKind),
    Escalate(EscalateTarget),
}

impl Route {
    pub const DESCRIPTORS: &'static [crate::blueprint_catalog::NodeDescriptor] =
        crate::blueprint_catalog::ROUTES;
    pub fn name(&self) -> &'static str {
        Self::DESCRIPTORS[match self {
            Self::Notify => 0,
            Self::Send(_) => 1,
            Self::Cron(_) => 2,
            Self::Command(_) => 3,
            Self::Sentinel(_) => 4,
            Self::Escalate(_) => 5,
            Self::RespondPrompt(_) => 6,
            Self::Stop => 7,
        }]
        .name
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "notify" => Ok(Self::Notify),
            "stop" => Ok(Self::Stop),
            "escalate:creator" => Ok(Self::Escalate(EscalateTarget::Creator)),
            _ if value.starts_with("escalate:agent:") => {
                let target = &value["escalate:agent:".len()..];
                if target.trim().is_empty()
                    || target.len() > 256
                    || target.chars().any(char::is_control)
                    || target.contains('/')
                    || !target
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
                {
                    return Err("route: escalation agent must be a bounded tab ID".into());
                }
                Ok(Self::Escalate(EscalateTarget::Agent(target.into())))
            }
            _ => {
                let (kind, target) = value.split_once(':').ok_or(
                    "route: use notify, blueprint:<exact-id>, send:<tab>, cron:<name>, respond_prompt:<kind>, stop, or escalate:creator|agent:<tab>",
                )?;
                if kind == "respond_prompt" {
                    if target.trim().is_empty()
                        || target.len() > 256
                        || target.chars().any(char::is_control)
                    {
                        return Err(
                            "route: respond_prompt kind must be nonempty and bounded".into()
                        );
                    }
                    return Ok(Self::RespondPrompt(
                        serde_json::from_value(serde_json::Value::String(target.into()))
                            .map_err(|_| "route: invalid respond_prompt kind")?,
                    ));
                }
                if target.trim().is_empty()
                    || target.len() > 256
                    || target.chars().any(char::is_control)
                    || target.contains(':')
                    || target.contains('/')
                {
                    return Err("route: target must be nonempty and bounded".into());
                }
                match kind {
                    "send" => Ok(Self::Send(target.into())),
                    "cron" => Ok(Self::Cron(target.into())),
                    "blueprint" | "sentinel"
                        if (target.starts_with("blueprint_")
                            || target.starts_with("sentinel_"))
                            && target
                                .chars()
                                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') =>
                    {
                        Ok(Self::Sentinel(target.into()))
                    }
                    _ => Err(
                        "route: use notify, blueprint:<exact-id>, send:<tab>, cron:<name>, or stop"
                            .into(),
                    ),
                }
            }
        }
    }
}

pub fn validate_classification(
    buckets: &[String],
    patterns: &BTreeMap<String, String>,
    connected: bool,
    interval: u64,
) -> Result<(), String> {
    let errors = classification_errors(buckets, patterns, connected, interval);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors
            .into_iter()
            .map(|(_, message)| message)
            .collect::<Vec<_>>()
            .join("; "))
    }
}

fn classification_errors(
    buckets: &[String],
    patterns: &BTreeMap<String, String>,
    connected: bool,
    interval: u64,
) -> Vec<(String, String)> {
    let mut errors = Vec::new();
    if interval < MIN_INTERVAL_MS {
        errors.push(("every".into(), "minimum blueprint interval is 10s".into()))
    }
    if buckets.is_empty() || buckets.len() > MAX_BUCKETS {
        errors.push(("buckets".into(), "provide between 1 and 16 buckets".into()))
    }
    let mut seen = BTreeSet::new();
    for (index, bucket) in buckets.iter().enumerate() {
        if bucket == "other"
            || bucket.is_empty()
            || bucket.len() > 64
            || !bucket
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
            || !seen.insert(bucket)
        {
            errors.push((format!("buckets[{index}]"),"unique names of up to 64 letters, numbers, underscores or dashes; other is automatic".into()))
        }
    }
    for (bucket, pattern) in patterns {
        let field = format!("matches.{bucket}");
        if !buckets.contains(bucket) || pattern.len() > 4096 {
            errors.push((
                field,
                "bucket must be declared and pattern bounded to 4096 bytes".into(),
            ));
            continue;
        }
        if let Err(error) = regex::Regex::new(pattern) {
            errors.push((field, error.to_string()))
        }
    }
    if !connected && patterns.is_empty() {
        errors.push((
            "matches".into(),
            "blueprint requires a connected Supporter Choice adapter or --match bucket=regex"
                .into(),
        ))
    }
    errors
}

pub fn duration_ms(value: &str) -> Result<u64, String> {
    if value.len() > 32
        || !value
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, 's' | 'm' | 'h' | 'd'))
    {
        return Err("duration: use seconds, minutes, hours or days, e.g. 20s or 1h30m".into());
    }
    if value.chars().any(|c| c.is_ascii_alphabetic())
        && value.ends_with(|c: char| c.is_ascii_digit())
    {
        return Err("duration: every component needs a unit".into());
    }
    let mut number = String::new();
    let mut bound = 0u64;
    for c in value.chars() {
        if c.is_ascii_digit() {
            number.push(c);
            continue;
        }
        let n: u64 = number.parse().map_err(|_| "duration: invalid number")?;
        number.clear();
        let factor = match c {
            's' => 1000,
            'm' => 60_000,
            'h' => 3_600_000,
            'd' => 86_400_000,
            _ => unreachable!(),
        };
        bound = bound
            .checked_add(n.checked_mul(factor).ok_or("duration: overflow")?)
            .ok_or("duration: overflow")?;
    }
    if !number.is_empty() {
        bound = bound
            .checked_add(
                number
                    .parse::<u64>()
                    .map_err(|_| "duration: invalid number")?
                    .checked_mul(60_000)
                    .ok_or("duration: overflow")?,
            )
            .ok_or("duration: overflow")?;
    }
    if bound == 0 || bound > 366 * 86_400_000 {
        return Err("duration: must be positive and at most 366 days".into());
    }
    Ok(bound)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub parameters: HandlerParameters,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandlerParameters {
    pub target: Option<String>,
    pub command: Option<String>,
    pub label: Option<String>,
    pub timeout_ms: Option<u64>,
}

impl Definition {
    pub fn resolve(self) -> Result<(String, Route), String> {
        let parameters = self.parameters;
        let has_command = parameters.command.is_some()
            || parameters.label.is_some()
            || parameters.timeout_ms.is_some();
        let action = if self.kind == "command" {
            if parameters.target.is_some() {
                return Err("command has no target parameter".into());
            }
            let command = CommandParameters {
                command: parameters.command.ok_or("command text is required")?,
                label: parameters.label.ok_or("command label is required")?,
                timeout_ms: parameters
                    .timeout_ms
                    .ok_or("command timeout_ms is required")?,
            };
            command.validate()?;
            Route::Command(command)
        } else {
            if has_command {
                return Err("command parameters require type=command".into());
            }
            match (self.kind.as_str(), parameters.target) {
                ("notify", None) => Route::Notify,
                ("stop", None) => Route::Stop,
                (
                    kind @ ("blueprint" | "sentinel" | "send" | "cron" | "respond_prompt"),
                    Some(target),
                ) => Route::parse(&format!("{kind}:{target}"))?,
                ("escalate", Some(target)) => Route::parse(&format!("escalate:{target}"))?,
                _ => return Err("handler type or parameters are invalid".into()),
            }
        };
        Ok((self.id, action))
    }
}

/// What a graph stage observes instead of running a shell probe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Observation {
    Tab(String),
    CronRun(String),
}

impl Observation {
    pub fn id(&self) -> &str {
        match self {
            Self::Tab(id) | Self::CronRun(id) => id,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Create {
    #[serde(default)]
    pub terminal: Option<crate::blueprint_terminal::Config>,
    #[serde(default)]
    pub session: bool,
    #[serde(default)]
    pub note: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub probe: Option<String>,
    #[serde(default)]
    pub observe: Option<Observation>,
    #[serde(default)]
    pub input: Option<crate::sentinel_input::InputSpec>,
    pub question: String,
    pub buckets: Vec<String>,
    #[serde(default = "default_every")]
    pub every: String,
    #[serde(default)]
    pub routes: BTreeMap<String, String>,
    #[serde(default)]
    pub matches: BTreeMap<String, String>,
    #[serde(default)]
    pub model_classification: bool,
    #[serde(default)]
    pub rearm_on_change: BTreeSet<String>,
    pub until: Option<String>,
    pub timeout: Option<String>,
    #[serde(default)]
    pub paused: bool,
    pub handlers: Option<BTreeMap<String, Vec<Definition>>>,
    #[serde(default)]
    pub escalation_timeout_ms: Option<u64>,
    pub max_transitions: Option<u16>,
    pub max_actions: Option<u16>,
}

fn default_every() -> String {
    "20s".into()
}

impl Create {
    pub fn normalize_handlers(&mut self) {
        let Some(buckets) = &mut self.handlers else {
            return;
        };
        let mut used = buckets
            .values()
            .flatten()
            .filter(|handler| !handler.id.is_empty())
            .map(|handler| handler.id.clone())
            .collect::<BTreeSet<_>>();
        for (bucket, handlers) in buckets {
            for handler in handlers {
                if handler.id.is_empty() {
                    for ordinal in 1..=65 {
                        let id = crate::automation_name::generated_handler_id(
                            &handler.kind,
                            handler.parameters.target.as_deref(),
                            handler.parameters.label.as_deref(),
                            bucket,
                            ordinal,
                        );
                        if used.insert(id.clone()) {
                            handler.id = id;
                            break;
                        }
                    }
                }
            }
        }
    }

    pub fn admission_errors(&self) -> Vec<(String, String)> {
        let mut errors = Vec::new();
        let mut add = |field: &str, message: String| errors.push((field.into(), message));
        if let Some(name) = &self.name {
            if let Err(error) = crate::automation_name::validate_sentinel_name(name) {
                add("name", error)
            }
        } else {
            add("name", "A Blueprint name is required".into())
        }
        if let Err(error) = crate::automation_name::validate_note(self.note.as_deref()) {
            add("note", error)
        }
        let sources = usize::from(self.probe.is_some())
            + usize::from(self.observe.is_some())
            + usize::from(self.input.is_some());
        if sources != 1 {
            add("", "Choose exactly one probe, observe or input".into())
        }
        if self.probe.as_ref().is_some_and(|probe| {
            probe.trim().is_empty() || probe.len() > 8192 || probe.contains('\0')
        }) {
            add("probe", "Probe requires 1..8192 bytes without NUL".into())
        }
        if self.observe.as_ref().is_some_and(|binding| {
            binding.id().is_empty()
                || binding.id().len() > 256
                || binding.id().chars().any(char::is_control)
        }) {
            add(
                "observe",
                "Observation requires a bounded tab or cron-run ID".into(),
            )
        }
        if let Some(input) = &self.input {
            if let Err(error) = crate::sentinel_input::validate(input) {
                add("input", error.to_string())
            }
        }
        if let Some(terminal) = &self.terminal {
            if let Err(error) = terminal.validate() {
                add("terminal", error)
            }
            if !matches!(self.observe, Some(Observation::Tab(_))) {
                add(
                    "terminal",
                    "Terminal options require a tab observation".into(),
                )
            }
            if let Some(pattern) = &terminal.pattern {
                if let Err(error) = regex::Regex::new(pattern) {
                    add("terminal.pattern", error.to_string())
                }
            }
        }
        if self.question.trim().is_empty() || self.question.len() > 4096 {
            add("question", "Question requires 1..4096 bytes".into())
        }
        let interval = match duration_ms(&self.every) {
            Ok(interval) => interval,
            Err(error) => {
                add("every", error);
                MIN_INTERVAL_MS
            }
        };
        for (field, error) in classification_errors(&self.buckets, &self.matches, true, interval) {
            add(&field, error)
        }
        if let Some(timeout) = &self.timeout {
            if let Err(error) = duration_ms(timeout) {
                add("timeout", error)
            }
        }
        if self
            .until
            .as_ref()
            .is_some_and(|bucket| !self.buckets.contains(bucket))
        {
            add("until", "Name a declared bucket".into())
        }
        for bucket in &self.rearm_on_change {
            if !self.buckets.contains(bucket) {
                add("rearm_on_change", format!("Unknown bucket {bucket}"))
            }
        }
        if self.input.is_some() && !self.rearm_on_change.is_empty() {
            add(
                "rearm_on_change",
                "Rearming requires a probe or observation source".into(),
            )
        }
        if !self.routes.is_empty() && self.handlers.is_some() {
            add("routes", "Choose routes or handlers, not both".into())
        }
        for (bucket, route) in &self.routes {
            if !self.buckets.contains(bucket) {
                add(&format!("routes.{bucket}"), "Name a declared bucket".into())
            }
            if route != "none" {
                if let Err(error) = Route::parse(route) {
                    add(&format!("routes.{bucket}"), error)
                }
            }
        }
        let mut ids = BTreeSet::new();
        let mut total = 0;
        let mut other_count = 0;
        let mut escalations = 0;
        if let Some(buckets) = &self.handlers {
            for (bucket, handlers) in buckets {
                if bucket != "other" && !self.buckets.contains(bucket) {
                    add(
                        &format!("handlers.{bucket}"),
                        "Name a declared bucket or other".into(),
                    )
                }
                if handlers.len() > 16 {
                    add(
                        &format!("handlers.{bucket}"),
                        "At most 16 handlers per bucket".into(),
                    )
                }
                for (index, handler) in handlers.iter().enumerate() {
                    total += 1;
                    let path = format!("handlers.{bucket}[{index}]");
                    if handler.id.is_empty()
                        || handler.id.len() > 64
                        || !handler
                            .id
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
                        || !ids.insert(&handler.id)
                    {
                        add(
                            &format!("{path}.id"),
                            "Use a unique bounded handler ID or omit it".into(),
                        )
                    }
                    match handler.clone().resolve() {
                        Err(error) => add(&format!("{path}.parameters"), error),
                        Ok((_, route)) => {
                            if bucket == "other" {
                                other_count += 1;
                                if matches!(route, Route::Escalate(_)) {
                                    escalations += 1
                                }
                                if !matches!(route, Route::Notify | Route::Escalate(_)) {
                                    add(&path, "Other handlers may only notify or escalate".into())
                                }
                            } else if matches!(route, Route::Escalate(_)) {
                                add(&path, "Escalation is allowed only on other".into())
                            }
                        }
                    }
                }
            }
        }
        if total > 64 {
            add("handlers", "At most 64 handlers per Blueprint".into())
        }
        if other_count > 4
            || escalations > 1
            || ((other_count == 0) != self.escalation_timeout_ms.is_none())
            || self.escalation_timeout_ms == Some(0)
        {
            add("escalation_timeout_ms","Other handlers require a positive timeout, at most four handlers and one escalation".into())
        }
        let composition = self.paused
            || self.handlers.is_some()
            || self
                .routes
                .values()
                .any(|route| route.starts_with("blueprint:"));
        if composition && self.timeout.is_none() {
            add(
                "timeout",
                "Composition watches require an explicit timeout".into(),
            )
        }
        errors
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreationOptions {
    pub timeout_ms: Option<u64>,
    pub paused: bool,
}
