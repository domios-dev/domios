use std::time::Instant;

pub(super) struct HookTiming {
    method: &'static str,
    started: Instant,
    stage_started: Instant,
    stage: &'static str,
    pub bytes: usize,
}

impl HookTiming {
    pub fn new(method: &'static str) -> Self {
        let started = Instant::now();
        Self {
            method,
            started,
            stage_started: started,
            stage: "prepare",
            bytes: 0,
        }
    }

    pub fn enter(&mut self, stage: &'static str) {
        self.record();
        self.stage = stage;
        self.stage_started = Instant::now();
    }

    fn record(&self) {
        log::debug!(
            "[runtime-hook] method={} stage={} stage_us={} total_us={} argument_bytes={}",
            self.method,
            self.stage,
            self.stage_started.elapsed().as_micros(),
            self.started.elapsed().as_micros(),
            self.bytes
        );
    }
}

impl Drop for HookTiming {
    fn drop(&mut self) {
        self.record();
    }
}

pub(super) fn serialized_bytes(value: &serde_json::Value) -> usize {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len());
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    let _ = serde_json::to_writer(&mut counter, value);
    counter.0
}
