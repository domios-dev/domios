use parking_lot::Mutex;
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::Arc;

pub trait DaemonQuestionHandler: Send + Sync {
    fn answer(&self, request: &str, label: &str) -> Option<Result<Value, String>>;
}

pub struct DaemonQuestions {
    pub handler: Arc<dyn DaemonQuestionHandler>,
    active: Mutex<BTreeSet<String>>,
}

impl DaemonQuestions {
    pub fn new(handler: Arc<dyn DaemonQuestionHandler>) -> Self {
        Self {
            handler,
            active: Mutex::new(BTreeSet::new()),
        }
    }
    pub fn claim(self: &Arc<Self>, id: &str) -> Option<QuestionClaim> {
        let mut active = self.active.lock();
        if active.len() >= 256 || !active.insert(id.into()) {
            return None;
        }
        Some(QuestionClaim {
            owner: self.clone(),
            id: id.into(),
        })
    }
}

pub struct QuestionClaim {
    owner: Arc<DaemonQuestions>,
    id: String,
}
impl Drop for QuestionClaim {
    fn drop(&mut self) {
        self.owner.active.lock().remove(&self.id);
    }
}

#[cfg(test)]
mod tests;
