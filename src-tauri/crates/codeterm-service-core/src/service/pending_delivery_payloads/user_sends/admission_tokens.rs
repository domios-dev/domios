use parking_lot::{Condvar, Mutex};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAX_TOKENS: usize = 256;

pub(super) struct TokenEntry {
    pub(super) send_id: String,
    pub(super) sequence: u64,
    done: Mutex<bool>,
    changed: Condvar,
}

#[derive(Default)]
pub(super) struct AdmissionTokens {
    entries: HashMap<String, Arc<TokenEntry>>,
    order: VecDeque<String>,
}

impl AdmissionTokens {
    pub(super) fn existing(&self, token: &str) -> Option<Arc<TokenEntry>> {
        self.entries.get(token).cloned()
    }

    pub(super) fn insert(
        &mut self,
        token: String,
        send_id: String,
        sequence: u64,
    ) -> Result<Arc<TokenEntry>, String> {
        while self.entries.len() >= MAX_TOKENS {
            let Some(position) = self.order.iter().position(|token| {
                self.entries
                    .get(token)
                    .is_some_and(|entry| *entry.done.lock())
            }) else {
                return Err("user token retention capacity reached".into());
            };
            let old = self
                .order
                .remove(position)
                .expect("retained token position");
            self.entries.remove(&old);
        }
        let entry = Arc::new(TokenEntry {
            send_id,
            sequence,
            done: Mutex::new(false),
            changed: Condvar::new(),
        });
        self.order.push_back(token.clone());
        self.entries.insert(token, entry.clone());
        Ok(entry)
    }
}

impl TokenEntry {
    pub(super) fn finish(&self) {
        *self.done.lock() = true;
        self.changed.notify_all();
    }

    pub(super) fn wait(&self) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut done = self.done.lock();
        while !*done {
            if self.changed.wait_until(&mut done, deadline).timed_out() && !*done {
                return Err("admission_unknown: original token admission is still pending".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_tokens_are_evicted_to_allow_fresh_admissions_with_bounded_memory() {
        let mut tokens = AdmissionTokens::default();
        for index in 0..=MAX_TOKENS {
            tokens
                .insert(
                    format!("token-{index}"),
                    format!("send-{index}"),
                    index as u64,
                )
                .unwrap()
                .finish();
        }
        assert!(tokens.existing("token-0").is_none());
        assert_eq!(tokens.entries.len(), MAX_TOKENS);
        assert!(tokens.existing(&format!("token-{MAX_TOKENS}")).is_some());
    }
}
