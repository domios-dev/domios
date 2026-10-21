use std::any::{Any, TypeId};
use std::collections::{hash_map::Entry, HashMap};
use std::sync::Arc;

type HostValue = Arc<dyn Any + Send + Sync>;

#[derive(Default)]
pub(super) struct HostExtensions(parking_lot::Mutex<HashMap<TypeId, HostValue>>);

impl HostExtensions {
    fn register<T: Any + Send + Sync>(&self, value: Arc<T>) -> Result<(), Arc<T>> {
        match self.0.lock().entry(TypeId::of::<T>()) {
            Entry::Vacant(entry) => {
                entry.insert(value);
                Ok(())
            }
            Entry::Occupied(_) => Err(value),
        }
    }

    fn get<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        let value = self.0.lock().get(&TypeId::of::<T>()).cloned()?;
        value.downcast::<T>().ok()
    }
}

impl super::ServiceContext {
    /// Registers one host-owned service per type. Tauri types must never be registered.
    pub fn register_host_extension<T: Any + Send + Sync>(
        &self,
        value: Arc<T>,
    ) -> Result<(), Arc<T>> {
        self.host_extensions.register(value)
    }

    pub fn host_extension<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        self.host_extensions.get()
    }
}

#[cfg(test)]
mod tests;
