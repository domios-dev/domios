use std::{future::Future, time::Duration};

pub(crate) fn deadline<F: Future>(
    duration: Duration,
    future: F,
) -> Result<F::Output, tokio::time::error::Elapsed> {
    crate::async_runtime::block_on(async { tokio::time::timeout(duration, future).await })
}

pub(crate) fn task<F>(future: F) -> Result<F::Output, String>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let current = tokio::runtime::Handle::try_current().ok();
    if current.as_ref().is_some_and(|handle| {
        handle.id() == crate::async_runtime::handle().id()
            && handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::CurrentThread
    }) {
        return Err(
            "Use the asynchronous prompt response on a current-thread service runtime".into(),
        );
    }
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    crate::async_runtime::spawn(async move {
        let result = future.await;
        let _ = send.send(result);
    });
    let receive = || {
        receive
            .recv()
            .map_err(|error| format!("Runtime response task failed: {error}"))
    };
    if current
        .is_some_and(|handle| handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread)
    {
        tokio::task::block_in_place(receive)
    } else {
        receive()
    }
}

#[cfg(test)]
mod tests;
