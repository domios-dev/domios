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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cached_snapshot_returns_without_waiting_for_fetch() {
        let (send, receive) = tokio::sync::oneshot::channel::<u32>();
        let job = std::sync::Mutex::new(None);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            serve(
                7,
                false,
                || async { Ok(receive.await.unwrap()) },
                |refresh| {
                    *job.lock().unwrap() = Some(refresh);
                },
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result, 7);
        send.send(9).unwrap();
        assert_eq!(job.into_inner().unwrap().unwrap().await.unwrap(), 9);
    }

    #[tokio::test]
    async fn forced_refresh_returns_the_fetched_snapshot() {
        assert_eq!(
            serve(
                7,
                true,
                || async { Ok(9) },
                |_| panic!("forced refresh queued")
            )
            .await
            .unwrap(),
            9
        );
    }
}
