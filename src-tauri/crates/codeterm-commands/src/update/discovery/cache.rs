use std::future::Future;

pub async fn serve<T, F, Fut>(
    cached: T,
    force: bool,
    refresh: F,
    background: impl FnOnce(Fut),
) -> Result<T, String>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, String>>,
{
    if force {
        refresh().await
    } else {
        background(refresh());
        Ok(cached)
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn fresh(checked_at_ms: Option<u64>, now_ms: u64) -> bool {
    checked_at_ms
        .and_then(|checked| now_ms.checked_sub(checked))
        .is_some_and(|age| age < 30_000)
}

#[cfg(test)]
mod tests;
