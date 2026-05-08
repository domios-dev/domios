//! Route table configuration and matching.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use anyhow::{Context, Result};
use regex::Regex;
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
            let table: RouteTable = toml::from_str(&contents)
                .context("Failed to parse routes file")?;
            Ok(table)
        } else {
            // Create default routes file with examples
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
        routes.insert("stripe-failed".to_string(), Route {
            name: "stripe-failed".to_string(),
            r#match: RouteMatch {
                path: Some("/webhook/stripe".to_string()),
                header: HashMap::from_iter([
                    ("X-Stripe-Event".to_string(), "payment_intent.payment_failed".to_string()),
                ]),
                json_path: None,
            },
            target: RouteTarget {
                peer: "BLACKSTAR".to_string(),
                pane: "billing".to_string(),
            },
            template: "Stripe failure: ${{json.data.object.amount}} for {{json.data.object.metadata.user_email}}".to_string(),
            auth: Some(WebhookAuth {
                auth_type: "stripe".to_string(),
                secret: "${STRIPE_WEBHOOK_SECRET}".to_string(),
            }),
        });

        // GitHub PR example
        routes.insert("github-pr".to_string(), Route {
            name: "github-pr".to_string(),
            r#match: RouteMatch {
                path: Some("/webhook/github".to_string()),
                header: HashMap::from_iter([
                    ("X-GitHub-Event".to_string(), "pull_request".to_string()),
                ]),
                json_path: None,
            },
            target: RouteTarget {
                peer: "BLACKSTAR".to_string(),
                pane: "general".to_string(),
            },
            template: "New PR: {{json.pull_request.html_url}} by @{{json.pull_request.user.login}}".to_string(),
            auth: Some(WebhookAuth {
                auth_type: "github".to_string(),
                secret: "${GITHUB_WEBHOOK_SECRET}".to_string(),
            }),
        });

        // GitHub push example
        routes.insert("github-push".to_string(), Route {
            name: "github-push".to_string(),
            r#match: RouteMatch {
                path: Some("/webhook/github".to_string()),
                header: HashMap::from_iter([
                    ("X-GitHub-Event".to_string(), "push".to_string()),
                ]),
                json_path: None,
            },
            target: RouteTarget {
                peer: "BLACKSTAR".to_string(),
                pane: "ci".to_string(),
            },
            template: "Push to {{json.repository.full_name}}: {{json.commits.0.message}}".to_string(),
            auth: Some(WebhookAuth {
                auth_type: "github".to_string(),
                secret: "${GITHUB_WEBHOOK_SECRET}".to_string(),
            }),
        });

        Self { routes }
    }

    /// Find matching route for a webhook request.
    pub fn match_request<'a>(
        &'a self,
        path: &str,
        headers: &'a HashMap<String, String>,
        body: &serde_json::Value,
    ) -> Option<(&'a Route, Vec<String>)> {
        for route in self.routes.values() {
            if let Some(matches) = route.matches(path, headers, body) {
                return Some((route, matches));
            }
        }
        None
    }
}

impl Route {
    /// Check if this route matches the request.
    pub fn matches(
        &self,
        path: &str,
        headers: &HashMap<String, String>,
        body: &serde_json::Value,
    ) -> Option<Vec<String>> {
        let mut errors = Vec::new();

        // Check path
        if let Some(ref match_path) = self.r#match.path {
            if !path.starts_with(match_path) {
                return None;
            }
        }

        // Check headers
        for (header, expected) in &self.r#match.header {
            let header_key = header.replace('_', "-"); // Allow underscore in config
            match headers.get(header_key).or_else(|| headers.get(header)) {
                Some(actual) if actual == expected => {}
                actual => {
                    errors.push(format!(
                        "Header {} mismatch: expected '{}', got '{:?}'",
                        header, expected, actual
                    ));
                }
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

        if errors.is_empty() {
            Some(errors)
        } else if errors.len() == 1 && self.r#match.header.is_empty() {
            // Single JSON path mismatch is acceptable for selective matching
            None
        } else {
            None
        }
    }
}