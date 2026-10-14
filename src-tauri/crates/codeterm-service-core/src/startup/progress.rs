use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Boundary {
    Started,
    Completed,
}

pub(crate) struct StartupStep<F = fn(Boundary, &'static str, Option<Duration>)>
where
    F: Fn(Boundary, &'static str, Option<Duration>),
{
    name: &'static str,
    started: Instant,
    sink: F,
}

impl StartupStep {
    pub(crate) fn logged(name: &'static str) -> Self {
        Self::with_sink(name, log_boundary)
    }
}

impl<F> StartupStep<F>
where
    F: Fn(Boundary, &'static str, Option<Duration>),
{
    pub(crate) fn with_sink(name: &'static str, sink: F) -> Self {
        sink(Boundary::Started, name, None);
        Self {
            name,
            started: Instant::now(),
            sink,
        }
    }
}

impl<F> Drop for StartupStep<F>
where
    F: Fn(Boundary, &'static str, Option<Duration>),
{
    fn drop(&mut self) {
        (self.sink)(Boundary::Completed, self.name, Some(self.started.elapsed()));
    }
}

fn log_boundary(boundary: Boundary, name: &'static str, elapsed: Option<Duration>) {
    match boundary {
        Boundary::Started => log::info!("[startup] step started: {name}"),
        Boundary::Completed => log::info!(
            "[startup] step completed: {name} ({}ms)",
            elapsed.unwrap_or_default().as_millis()
        ),
    }
}
