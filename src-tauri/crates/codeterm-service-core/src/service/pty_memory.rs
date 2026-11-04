use super::ServiceContext;
use std::collections::HashSet;

impl ServiceContext {
    pub(super) fn retire_pty_memory(&self, generations: &[(String, Option<u64>)]) {
        let retained: HashSet<String> = self.agent_run_pty_map.lock().values().cloned().collect();
        let mut output = self.output_buf.lock();
        for (id, generation) in generations {
            let retired = self
                .screen_state
                .remove_matching_generation(id, *generation)
                || self.screen_state.pty_generation(id).is_none();
            if retired && !retained.contains(id) {
                output.remove(id);
            }
        }
    }
}

#[cfg(test)]
mod tests;
