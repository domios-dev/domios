use std::collections::HashMap;

#[derive(Default)]
pub(super) struct BubbleCache<T> {
    entries: HashMap<String, Entry<T>>,
}

struct Entry<T> {
    value: Option<T>,
    running: bool,
    invalidated: bool,
}

impl<T: Clone> BubbleCache<T> {
    pub fn request(&mut self, key: &str, invalidated: bool) -> (Option<T>, bool) {
        if !self.entries.contains_key(key) && self.entries.len() >= 256 {
            self.entries.retain(|_, entry| entry.running);
            if self.entries.len() >= 256 {
                return (None, false);
            }
        }
        let entry = self.entries.entry(key.to_owned()).or_insert(Entry {
            value: None,
            running: false,
            invalidated: false,
        });
        if entry.running {
            entry.invalidated |= invalidated;
            return (entry.value.clone(), false);
        }
        let refresh = entry.value.is_none() || invalidated;
        entry.running = refresh;
        (entry.value.clone(), refresh)
    }

    pub fn finish(&mut self, key: &str, value: Option<T>) -> bool {
        let Some(entry) = self.entries.get_mut(key) else {
            return false;
        };
        if let Some(value) = value {
            entry.value = Some(value);
        }
        entry.running = std::mem::take(&mut entry.invalidated);
        entry.running
    }
}

#[cfg(test)]
mod tests;
