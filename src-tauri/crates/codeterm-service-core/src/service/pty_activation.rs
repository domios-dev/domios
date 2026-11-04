mod admission;
use admission::Admission;
use tokio::sync::OwnedSemaphorePermit;

pub struct TabActivationPermit {
    _owner: OwnedSemaphorePermit,
    _admission: Admission,
}

#[cfg(any(test, not(feature = "test-support")))]
pub fn acquire(tab_id: &str) -> Result<TabActivationPermit, String> {
    futures_executor::block_on(Admission::new(tab_id)?.acquire(|| {}))
}

#[cfg(any(test, feature = "test-support"))]
pub fn acquire_with_wait_hook(
    tab_id: &str,
    waiting: impl FnMut(),
) -> Result<TabActivationPermit, String> {
    futures_executor::block_on(Admission::new(tab_id)?.acquire(waiting))
}

#[cfg(any(test, not(feature = "test-support")))]
pub async fn acquire_async(tab_id: String) -> Result<TabActivationPermit, String> {
    Admission::new(&tab_id)?.acquire(|| {}).await
}

#[cfg(any(test, feature = "test-support"))]
pub async fn acquire_async_with_wait_hook(
    tab_id: String,
    waiting: impl FnMut(),
) -> Result<TabActivationPermit, String> {
    Admission::new(&tab_id)?.acquire(waiting).await
}

#[cfg(test)]
mod tests;
