//! Route table configuration and matching.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use anyhow::{Context, Result};
use jsonpath::JsonPath;

/// A route defines how incoming webhooks are matched and where they're sent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    /// Route name (e.g., "stripe-failed")
    pub name: String,

    /// Match conditions
    #[serde(default)]
    pub r#match: RouteMatch,

    /// Target peer and pane
    pub target: RouteTarget,

    /// Template for the message
    pub template: String,

    /// Optional webhook authentication
    #[serde(default)]
    pub auth: Option<WebhookAuth>,
}

impl Route {
    /// Builder pattern for creating routes.
    pub fn builder(name: impl Into<String>) -> RouteBuilder {
        RouteBuilder::new(name)
    }

    /// Check if this route matches the request.
    pub fn matches(
        &self,
        path: &str,
        headers: &HashMap<String, String>,
        body: &serde_json::Value,
    ) -> Option<Vec<String>> {
        // Check path
        if let Some(ref match_path) = self.r#match.path {
            if !path.starts_with(match_path) {
                return None;
            }
        }

        // Check headers
        for (header, expected) in &self.r#match.header {
            let header_key = header.replace('_', "-");
            let actual = headers.get(&header_key).or_else(|| headers.get(header));
            if actual != Some(expected) {
                return None;
            }
        }

        // Check JSON path
        if let Some(ref json_match) = self.r#match.json_path {
            let path_expr = JsonPath::compile(&json_match.path).ok()?;
            let result = path_expr.read(body).ok()?;
            let actual = result.as_str().unwrap_or("");
            if actual != json_match.value {
                return None;
            }
        }

        Some(Vec::new())
    }
}

/// Builder for creating routes with a fluent API.
#[derive(Debug)]
pub struct RouteBuilder {
    name: String,
    match_conditions: RouteMatch,
    target: Option<RouteTarget>,
    template: Option<String>,
    auth: Option<WebhookAuth>,
}

impl RouteBuilder {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            match_conditions: RouteMatch::default(),
            target: None,
            template: None,
            auth: None,
        }
    }

    /// Set path to match.
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.match_conditions.path = Some(path.into());
        self
    }

    /// Add a header match.
    pub fn header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.match_conditions.header.insert(key.into(), value.into());
        self
    }

    /// Set JSON path match.
    pub fn json_path(mut self, path: impl Into<String>, value: impl Into<String>) -> Self {
        self.match_conditions.json_path = Some(JsonPathMatch {
            path: path.into(),
            value: value.into(),
        });
        self
    }

    /// Set target peer and pane.
    pub fn target(mut self, peer: impl Into<String>, pane: impl Into<String>) -> Self {
        self.target = Some(RouteTarget {
            peer: peer.into(),
            pane: pane.into(),
        });
        self
    }

    /// Set message template.
    pub fn template(mut self, template: impl Into<String>) -> Self {
        self.template = Some(template.into());
        self
    }

    /// Set webhook authentication.
    pub fn auth(mut self, auth_type: impl Into<String>, secret: impl Into<String>) -> Self {
        self.auth = Some(WebhookAuth {
            auth_type: auth_type.into(),
            secret: secret.into(),
        });
        self
    }

    /// Build the route, panicking if required fields are missing.
    pub fn build(self) -> Route {
        Route {
            name: self.name,
            r#match: self.match_conditions,
            target: self.target.expect("target is required"),
            template: self.template.expect("template is required"),
            auth: self.auth,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RouteMatch {
    /// URL path prefix to match
    pub path: Option<String>,

    /// Header matches (exact value)
    #[serde(default)]
    pub header: HashMap<String, String>,

    /// JSON path match (JSONPath expression and expected value)
    #[serde(default)]
    pub json_path: Option<JsonPathMatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonPathMatch {
    /// JSONPath expression (e.g., "$.type")
    pub path: String,
    /// Expected value
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteTarget {
    /// Target peer instance_id or instance_name
    pub peer: String,
    /// Target pane id or name
    pub pane: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookAuth {
    /// Auth type: hmac_sha256, github, stripe
    #[serde(rename = "type")]
    pub auth_type: String,
    /// Secret for verification
    pub secret: String,
}

/// Route table containing all routes.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RouteTable {
    #[serde(default)]
    pub routes: HashMap<String, Route>,
}

impl RouteTable {
    /// Load route table from file or create default.
    pub fn load_or_create(path: &Path) -> Result<Self> {
        if path.exists() {
            let contents = std::fs::read_to_string(path)
                .context("Failed to read routes file")?;
            toml::from_str(&contents)
                .context("Failed to parse routes file")
        } else {
            let table = Self::default_example();
            let contents = toml::to_string_pretty(&table)
                .context("Failed to serialize routes")?;
            std::fs::write(path, &contents)
                .context("Failed to write routes file")?;
            Ok(table)
        }
    }

    /// Get default example routes.
    pub fn default_example() -> Self {
        let mut routes = HashMap::new();

        // Stripe webhook example
        routes.insert("stripe-failed".into(), Route::builder("stripe-failed")
            .path("/webhook/stripe")
            .header("X-Stripe-Event", "payment_intent.payment_failed")
            .target("BLACKSTAR", "billing")
            .template("Stripe failure: ${{json.data.object.amount}} for {{json.data.object.metadata.user_email}}")
            .auth("stripe", "${STRIPE_WEBHOOK_SECRET}")
            .build());

        // GitHub PR example
        routes.insert("github-pr".into(), Route::builder("github-pr")
            .path("/webhook/github")
            .header("X-GitHub-Event", "pull_request")
            .target("BLACKSTAR", "general")
            .template("New PR: {{json.pull_request.html_url}} by @{{json.pull_request.user.login}}")
            .auth("github", "${GITHUB_WEBHOOK_SECRET}")
            .build());

        // GitHub push example
        routes.insert("github-push".into(), Route::builder("github-push")
            .path("/webhook/github")
            .header("X-GitHub-Event", "push")
            .target("BLACKSTAR", "ci")
            .template("Push to {{json.repository.full_name}}: {{json.commits.0.message}}")
            .auth("github", "${GITHUB_WEBHOOK_SECRET}")
            .build());

        Self { routes }
    }

    /// Find matching route for a webhook request.
    pub fn match_request<'a>(
        &'a self,
        path: &str,
        headers: &'a HashMap<String, String>,
        body: &serde_json::Value,
    ) -> Option<(&'a Route, Vec<String>)> {
        self.routes.values()
            .find_map(|route| route.matches(path, headers, body)
                .map(|errors| (route, errors)))
    }
}
