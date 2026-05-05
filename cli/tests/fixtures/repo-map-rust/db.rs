use std::collections::HashMap;

pub trait Storage {
    fn get(&self, key: &str) -> Option<String>;
    fn put(&mut self, key: String, val: String);
}

pub struct MemoryStore {
    map: HashMap<String, String>,
}

impl Storage for MemoryStore {
    fn get(&self, key: &str) -> Option<String> {
        self.map.get(key).cloned()
    }
    fn put(&mut self, key: String, val: String) {
        self.map.insert(key, val);
    }
}

impl MemoryStore {
    pub fn new() -> Self {
        MemoryStore { map: HashMap::new() }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }
}
