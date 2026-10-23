use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;

pub type Reload<'a> = dyn FnMut() -> Result<(), String> + 'a;
pub type InstallOperation<'a> = dyn FnMut(&mut Reload<'_>) -> Result<(), String> + 'a;

pub trait InstallLifecycle: Send + Sync {
    fn install(
        &self,
        id: &str,
        destination: &Path,
        operation: &mut InstallOperation<'_>,
    ) -> Result<(), String>;
}

thread_local! {
    static LIFECYCLE: RefCell<Option<Arc<dyn InstallLifecycle>>> = RefCell::default();
}

struct LifecycleScope(Option<Arc<dyn InstallLifecycle>>);

impl Drop for LifecycleScope {
    fn drop(&mut self) {
        LIFECYCLE.with(|current| *current.borrow_mut() = self.0.take());
    }
}

pub fn with_install_lifecycle<T>(
    lifecycle: Arc<dyn InstallLifecycle>,
    operation: impl FnOnce() -> T,
) -> T {
    let previous = LIFECYCLE.with(|current| current.borrow_mut().replace(lifecycle));
    let _scope = LifecycleScope(previous);
    operation()
}

pub(super) fn activate(id: &str, staging: &Path, destination: &Path) -> Result<(), String> {
    let lifecycle = LIFECYCLE.with(|current| current.borrow().clone());
    match lifecycle {
        Some(lifecycle) => lifecycle.install(id, destination, &mut |reload| {
            super::install_swap::activate_with_reload(staging, destination, reload)
        }),
        None => super::install_swap::activate(staging, destination),
    }
}
