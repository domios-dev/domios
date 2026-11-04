pub(super) struct RegistryTiming<'a> {
    #[cfg(debug_assertions)]
    operation: &'static str,
    provider: &'a str,
    #[cfg(debug_assertions)]
    site: &'static std::panic::Location<'static>,
    #[cfg(debug_assertions)]
    started: std::time::Instant,
    #[cfg(debug_assertions)]
    acquired: Option<std::time::Instant>,
}

impl<'a> RegistryTiming<'a> {
    #[track_caller]
    pub(super) fn new(operation: &'static str, provider: &'a str) -> Self {
        #[cfg(not(debug_assertions))]
        let _ = operation;
        Self {
            #[cfg(debug_assertions)]
            operation,
            provider,
            #[cfg(debug_assertions)]
            site: std::panic::Location::caller(),
            #[cfg(debug_assertions)]
            started: std::time::Instant::now(),
            #[cfg(debug_assertions)]
            acquired: None,
        }
    }

    pub(super) fn acquired(self) -> Self {
        #[cfg(debug_assertions)]
        {
            let mut timing = self;
            timing.acquired = Some(std::time::Instant::now());
            timing
        }
        #[cfg(not(debug_assertions))]
        {
            self
        }
    }
}

impl Drop for RegistryTiming<'_> {
    fn drop(&mut self) {
        #[cfg(not(debug_assertions))]
        let _ = self.provider;
        #[cfg(debug_assertions)]
        {
            let Some(acquired) = self.acquired else {
                return;
            };
            let waited = acquired.duration_since(self.started);
            let held = acquired.elapsed();
            if waited.as_millis() >= 50 || held.as_millis() >= 50 {
                log::debug!(
                "[provider-registry] operation={} provider={} thread={:?} waited_us={} held_us={} at={}:{}",
                self.operation,
                self.provider,
                std::thread::current().id(),
                waited.as_micros(),
                held.as_micros(),
                self.site.file(),
                self.site.line()
            );
            }
        }
    }
}
