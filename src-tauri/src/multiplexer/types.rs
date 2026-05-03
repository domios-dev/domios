use serde::{Deserialize, Serialize};

/// A workspace contains multiple tabs and tracks which tab is active.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub tabs: Vec<Tab>,
    pub active_tab_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    /// Protected workspaces cannot be closed via API or UI.
    #[serde(default)]
    pub protected: bool,
}

/// A tab holds a split tree of panes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tab {
    pub id: String,
    pub title: String,
    pub split_tree: SplitNode,
}

/// Recursive split tree: either a leaf (single pane) or a split with children.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SplitNode {
    Leaf {
        pane_id: String,
    },
    Split {
        orientation: Orientation,
        /// Position of divider, 0.0 to 1.0.
        ratio: f64,
        children: Vec<SplitNode>,
    },
}

impl SplitNode {
    /// Check if this subtree contains a pane with the given ID.
    pub fn contains_pane(&self, target_pane_id: &str) -> bool {
        match self {
            SplitNode::Leaf { pane_id } => pane_id == target_pane_id,
            SplitNode::Split { children, .. } => {
                children.iter().any(|c| c.contains_pane(target_pane_id))
            }
        }
    }

    /// Collect all pane IDs in this subtree.
    pub fn collect_pane_ids(&self) -> Vec<String> {
        match self {
            SplitNode::Leaf { pane_id } => vec![pane_id.clone()],
            SplitNode::Split { children, .. } => {
                children.iter().flat_map(|c| c.collect_pane_ids()).collect()
            }
        }
    }

    /// Find and replace a leaf node containing `target_pane_id` with a split
    /// containing the old leaf and a new leaf. Returns true if replacement was made.
    pub fn split_leaf(
        &mut self,
        target_pane_id: &str,
        new_pane_id: &str,
        orientation: Orientation,
    ) -> bool {
        match self {
            SplitNode::Leaf { pane_id } if pane_id == target_pane_id => {
                *self = SplitNode::Split {
                    orientation,
                    ratio: 0.5,
                    children: vec![
                        SplitNode::Leaf {
                            pane_id: target_pane_id.to_string(),
                        },
                        SplitNode::Leaf {
                            pane_id: new_pane_id.to_string(),
                        },
                    ],
                };
                true
            }
            SplitNode::Split { children, .. } => {
                children.iter_mut().any(|c| c.split_leaf(target_pane_id, new_pane_id, orientation))
            }
            _ => false,
        }
    }

    /// Remove a leaf with the given pane_id. Returns true if the tree was modified.
    /// After removal, collapses parent splits that have only one child.
    pub fn remove_leaf(&mut self, target_pane_id: &str) -> bool {
        match self {
            SplitNode::Leaf { pane_id } => pane_id == target_pane_id,
            SplitNode::Split { children, .. } => {
                // First, check if any child is the target leaf
                let removed_idx = children.iter().position(|c| {
                    matches!(c, SplitNode::Leaf { pane_id } if pane_id == target_pane_id)
                });

                if let Some(idx) = removed_idx {
                    children.remove(idx);
                    // Collapse if only one child remains
                    if children.len() == 1 {
                        *self = children.remove(0);
                    }
                    return true;
                }

                // Recurse into child splits
                for child in children.iter_mut() {
                    if child.remove_leaf(target_pane_id) {
                        return true;
                    }
                }

                false
            }
        }
    }
}

/// Split orientation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum Orientation {
    /// Side by side (left | right).
    Horizontal,
    /// Top and bottom (top / bottom).
    Vertical,
}

/// Agent role in the orchestration hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AgentRole {
    /// Top-level orchestrator (depth 1). Can spawn Leads and Workers.
    Orchestrator,
    /// Lead spawned by orchestrator (depth 2). Can spawn Workers.
    Lead,
    /// Worker (depth 2 or 3). Executes tasks, reports back. Cannot spawn.
    Worker,
}

// Keep backward-compatible aliases for deserialization
impl AgentRole {
    pub fn from_str_compat(s: &str) -> Option<Self> {
        match s {
            "Orchestrator" | "Conductor" => Some(Self::Orchestrator),
            "Lead" => Some(Self::Lead),
            "Worker" | "SubWorker" => Some(Self::Worker),
            _ => None,
        }
    }
}

/// An interactive prompt detected in agent output (e.g. permission request).
/// Provider-agnostic — any agent can have pending prompts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptOption {
    pub label: String,
    /// Value to send to PTY (e.g. "1", "y", "n")
    pub value: String,
}

/// A pending interactive prompt from an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPrompt {
    pub question: String,
    pub options: Vec<PromptOption>,
}

/// A pane represents a single terminal or browser panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pane {
    pub id: String,
    /// Linked PTY ID, if this pane has a running terminal.
    pub pty_id: Option<String>,
    pub pane_type: PaneType,
    pub title: String,
    pub cwd: Option<String>,
    /// URL for browser panes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Agent type (e.g. "claude") — set by frontend when detected.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agent_type: Option<String>,
    /// Timestamp when agent_type was set — grace period for idle-poll exit detection.
    #[serde(skip, default)]
    pub agent_set_at: Option<std::time::Instant>,
    /// Agent session ID for resume (e.g. Claude Code session).
    #[serde(skip_serializing_if = "Option::is_none", default, alias = "claude_session_id")]
    pub agent_session_id: Option<String>,
    /// Orchestration role (orchestrator / lead / worker).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub role: Option<AgentRole>,
    /// Parent pane ID in the orchestration hierarchy.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parent_pane_id: Option<String>,
    /// Whether this pane can spawn child agents.
    #[serde(default)]
    pub can_spawn: bool,
    /// Depth in hierarchy: 0=none, 1=orchestrator, 2=lead/worker, 3=worker.
    #[serde(default)]
    pub depth: u8,
    /// Report target pane ID — for cron agents that report without joining hierarchy.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub report_target_pane_id: Option<String>,
    /// Pane id of the caller that ran `agent spawn` (CODETERM_PANE_ID at spawn time).
    /// Independent of `parent_pane_id` (which controls the role hierarchy and is
    /// forbidden for Orchestrators). Used as a final fallback by report routing
    /// so a top-level orchestrator's report still reaches its spawning agent.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub spawned_by_pane_id: Option<String>,
    /// Title naming priority level.
    #[serde(default)]
    pub title_level: TitleLevel,
    /// Whether chat view overlay is active for this pane.
    #[serde(default)]
    pub chat_view: bool,
    /// Pending interactive prompt (permission, confirmation, etc.)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pending_prompt: Option<PendingPrompt>,
    /// Auto-approve prompts (set when spawned with skip_permissions).
    /// When true, detected prompts are answered automatically instead of shown to user.
    #[serde(default)]
    pub auto_approve: bool,
    /// Protected panes cannot be closed via API or UI.
    #[serde(default)]
    pub protected: bool,
    /// Connected external services (e.g. ["telegram"]). Shown as icons next to pane title.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connected_services: Vec<String>,
    /// Hidden panes are excluded from default pane listings and TabBar.
    /// Used by graph-worker panes that live only in the memory panel.
    #[serde(default)]
    pub hidden: bool,
    /// Whether this pane should be rendered in the TabBar. Defaults to true.
    /// Graph-worker panes set this to false even when temporarily focused.
    #[serde(default = "default_show_in_tabbar")]
    pub show_in_tabbar: bool,
    /// Graph-worker id linking this pane to a GraphWorkerRegistry entry.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub worker_id: Option<String>,
}

fn default_show_in_tabbar() -> bool { true }

/// Title priority: higher levels cannot be overwritten by lower ones.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum TitleLevel {
    /// "Terminal" — initial default, lowest priority
    Default = 0,
    /// Auto-set by CWD/directory change (OSC 7)
    Auto = 1,
    /// Set by agent via OSC title (e.g. Claude sets task summary)
    Agent = 2,
    /// Manually set by user — highest priority, never overwritten
    User = 3,
}

impl Default for TitleLevel {
    fn default() -> Self { Self::Default }
}

/// Type of content displayed in a pane.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PaneType {
    Terminal,
    Browser,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leaf_collect_pane_ids() {
        let node = SplitNode::Leaf {
            pane_id: "pane-1".to_string(),
        };
        let ids = node.collect_pane_ids();
        assert_eq!(ids, vec!["pane-1"]);
    }

    #[test]
    fn test_split_collect_pane_ids() {
        let node = SplitNode::Split {
            orientation: Orientation::Horizontal,
            ratio: 0.5,
            children: vec![
                SplitNode::Leaf {
                    pane_id: "pane-1".to_string(),
                },
                SplitNode::Leaf {
                    pane_id: "pane-2".to_string(),
                },
            ],
        };
        let ids = node.collect_pane_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"pane-1".to_string()));
        assert!(ids.contains(&"pane-2".to_string()));
    }

    #[test]
    fn test_nested_split_collect_pane_ids() {
        let node = SplitNode::Split {
            orientation: Orientation::Horizontal,
            ratio: 0.5,
            children: vec![
                SplitNode::Leaf {
                    pane_id: "pane-1".to_string(),
                },
                SplitNode::Split {
                    orientation: Orientation::Vertical,
                    ratio: 0.5,
                    children: vec![
                        SplitNode::Leaf {
                            pane_id: "pane-2".to_string(),
                        },
                        SplitNode::Leaf {
                            pane_id: "pane-3".to_string(),
                        },
                    ],
                },
            ],
        };
        let ids = node.collect_pane_ids();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&"pane-1".to_string()));
        assert!(ids.contains(&"pane-2".to_string()));
        assert!(ids.contains(&"pane-3".to_string()));
    }

    #[test]
    fn test_split_leaf() {
        let mut node = SplitNode::Leaf {
            pane_id: "pane-1".to_string(),
        };
        let result = node.split_leaf("pane-1", "pane-2", Orientation::Horizontal);
        assert!(result);

        match &node {
            SplitNode::Split {
                orientation,
                ratio,
                children,
            } => {
                assert_eq!(*orientation, Orientation::Horizontal);
                assert_eq!(*ratio, 0.5);
                assert_eq!(children.len(), 2);
                let ids = node.collect_pane_ids();
                assert!(ids.contains(&"pane-1".to_string()));
                assert!(ids.contains(&"pane-2".to_string()));
            }
            _ => panic!("Expected Split node after split_leaf"),
        }
    }

    #[test]
    fn test_split_leaf_wrong_target() {
        let mut node = SplitNode::Leaf {
            pane_id: "pane-1".to_string(),
        };
        let result = node.split_leaf("pane-999", "pane-2", Orientation::Horizontal);
        assert!(!result);
        // Should remain a Leaf
        assert_eq!(node.collect_pane_ids(), vec!["pane-1"]);
    }

    #[test]
    fn test_split_leaf_nested() {
        // Start with a split of two leaves, then split one of them
        let mut node = SplitNode::Split {
            orientation: Orientation::Horizontal,
            ratio: 0.5,
            children: vec![
                SplitNode::Leaf {
                    pane_id: "pane-1".to_string(),
                },
                SplitNode::Leaf {
                    pane_id: "pane-2".to_string(),
                },
            ],
        };
        let result = node.split_leaf("pane-2", "pane-3", Orientation::Vertical);
        assert!(result);
        let ids = node.collect_pane_ids();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&"pane-3".to_string()));
    }

    #[test]
    fn test_remove_leaf_collapses() {
        let mut node = SplitNode::Split {
            orientation: Orientation::Horizontal,
            ratio: 0.5,
            children: vec![
                SplitNode::Leaf {
                    pane_id: "pane-1".to_string(),
                },
                SplitNode::Leaf {
                    pane_id: "pane-2".to_string(),
                },
            ],
        };
        let result = node.remove_leaf("pane-2");
        assert!(result);
        // Should collapse back to a single Leaf
        match &node {
            SplitNode::Leaf { pane_id } => assert_eq!(pane_id, "pane-1"),
            _ => panic!("Expected Leaf after removing one child from 2-child split"),
        }
    }

    #[test]
    fn test_remove_leaf_not_found() {
        let mut node = SplitNode::Leaf {
            pane_id: "pane-1".to_string(),
        };
        // remove_leaf on a leaf returns true if it matches (marks for removal by caller)
        // but does not self-destruct; this is handled at the caller level
        let result = node.remove_leaf("pane-999");
        assert!(!result);
    }

    #[test]
    fn test_nested_split_remove() {
        // pane-1 | (pane-2 / pane-3)
        let mut node = SplitNode::Split {
            orientation: Orientation::Horizontal,
            ratio: 0.5,
            children: vec![
                SplitNode::Leaf {
                    pane_id: "pane-1".to_string(),
                },
                SplitNode::Split {
                    orientation: Orientation::Vertical,
                    ratio: 0.5,
                    children: vec![
                        SplitNode::Leaf {
                            pane_id: "pane-2".to_string(),
                        },
                        SplitNode::Leaf {
                            pane_id: "pane-3".to_string(),
                        },
                    ],
                },
            ],
        };

        // Remove pane-3 from the inner split
        let result = node.remove_leaf("pane-3");
        assert!(result);
        // Inner split should collapse to just pane-2
        // Outer split: pane-1 | pane-2
        let ids = node.collect_pane_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"pane-1".to_string()));
        assert!(ids.contains(&"pane-2".to_string()));

        // Now remove pane-1 — should collapse to pane-2
        let result = node.remove_leaf("pane-1");
        assert!(result);
        match &node {
            SplitNode::Leaf { pane_id } => assert_eq!(pane_id, "pane-2"),
            _ => panic!("Expected Leaf after collapsing all splits"),
        }
    }

    #[test]
    fn test_split_preserves_orientation() {
        let mut node = SplitNode::Leaf {
            pane_id: "p1".to_string(),
        };
        node.split_leaf("p1", "p2", Orientation::Vertical);
        match &node {
            SplitNode::Split { orientation, .. } => {
                assert_eq!(*orientation, Orientation::Vertical);
            }
            _ => panic!("Expected Split"),
        }
    }
}
