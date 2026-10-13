use std::collections::{HashMap, HashSet};

type ItemKey = (String, String);

#[derive(Default)]
pub(super) struct RestoredItems {
    items: HashSet<ItemKey>,
    open: HashMap<ItemKey, String>,
}

impl RestoredItems {
    pub fn insert(&mut self, thread: &str, id: &str) {
        self.items.insert((thread.into(), id.into()));
    }

    pub fn open(&mut self, thread: &str, items: &HashMap<String, String>) {
        for (id, turn) in items {
            let key = (thread.into(), id.clone());
            if self.items.contains(&key) {
                self.open.insert(key, turn.clone());
            }
        }
    }

    pub fn suppresses(&self, key: &ItemKey) -> bool {
        self.items.contains(key)
    }

    pub fn completed(&mut self, key: &ItemKey) {
        self.open.remove(key);
    }

    pub fn finish_turn(&mut self, thread: &str, turn: &str) -> HashSet<String> {
        let ids = self
            .open
            .iter()
            .filter(|((owner, _), item_turn)| owner == thread && *item_turn == turn)
            .map(|((_, id), _)| id.clone())
            .collect();
        self.open
            .retain(|(owner, _), item_turn| owner != thread || item_turn != turn);
        ids
    }
}
