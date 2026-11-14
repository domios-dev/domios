use std::collections::HashSet;

pub struct LeaseModel<H> {
    handle: Option<H>,
    leases: HashSet<u64>,
    next_id: u64,
    generation: u64,
}

impl<H> Default for LeaseModel<H> {
    fn default() -> Self {
        Self {
            handle: None,
            leases: HashSet::new(),
            next_id: 0,
            generation: 0,
        }
    }
}

impl<H: Clone> LeaseModel<H> {
    pub fn begin_launch(&mut self) {
        self.generation += 1;
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn permits_cleanup(&self, generation: u64) -> bool {
        self.generation == generation
    }

    pub fn handle(&self) -> Option<H> {
        self.handle.clone()
    }

    pub fn install(&mut self, handle: H) {
        self.generation += 1;
        self.handle = Some(handle);
        self.leases.clear();
    }

    pub fn acquire(&mut self) -> Option<(u64, H)> {
        let handle = self.handle()?;
        self.next_id += 1;
        self.leases.insert(self.next_id);
        Some((self.next_id, handle))
    }

    pub fn release(&mut self, id: u64) -> Option<H> {
        if !self.leases.remove(&id) || !self.leases.is_empty() {
            return None;
        }
        self.handle.take()
    }

    pub fn retire(&mut self) -> Option<H> {
        self.leases.clear();
        self.handle.take()
    }
}

#[cfg(test)]
mod tests;
