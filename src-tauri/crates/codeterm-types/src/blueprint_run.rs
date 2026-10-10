use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parent {
    pub node: String,
    pub transition: String,
    pub handler: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub id: String,
    pub root_node: String,
    pub root_transition: String,
    pub started_at: u64,
    pub parent: Option<Parent>,
}

impl Context {
    pub fn root(node: &str, transition: &str, started_at: u64) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            root_node: node.into(),
            root_transition: transition.into(),
            started_at,
            parent: None,
        }
    }

    pub fn child(&self, node: &str, transition: &str, handler: &str) -> Self {
        Self {
            parent: Some(Parent {
                node: node.into(),
                transition: transition.into(),
                handler: handler.into(),
            }),
            ..self.clone()
        }
    }
}
