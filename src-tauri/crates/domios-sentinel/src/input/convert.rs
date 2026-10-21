use std::fs;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use codeterm_blueprint_types::sentinel_input::{MediaKind, MAX_CONVERTED_TEXT_BYTES};
use codeterm_plugin::converter::{
    output_within, ConversionCancellation, ConvertError, ConvertOutput, ConvertRequest, Step,
};

pub fn convert(
    registry: &codeterm_plugin::converter::ConverterRegistry,
    input: &Path,
    output_dir: &Path,
    from: MediaKind,
    to: MediaKind,
    deadline: Instant,
    cancellation: ConversionCancellation,
    cleanup: Option<std::sync::Arc<super::Cleanup>>,
    now: impl Fn() -> Instant + Send + 'static,
) -> Result<String, String> {
    if cancellation.is_cancelled() {
        return Err("conversion_cancelled".into());
    }
    let plan = registry.plan(from, to).map_err(|error| error.to_string())?;
    let (complete, result) = mpsc::channel();
    let worker_cancel = cancellation.clone();
    let input = input.to_path_buf();
    let output_dir = output_dir.to_path_buf();
    thread::spawn(move || {
        let cleanup_guard = cleanup;
        let value = convert_inner(&input, &output_dir, plan, deadline, worker_cancel.clone());
        drop(cleanup_guard);
        let _ = complete.send(value);
        worker_cancel.notify_change();
    });

    let mut failure = None;
    let mut completion_deadline = None;
    loop {
        if let Ok(value) = result.try_recv() {
            return failure.map_or(value, Err);
        }
        if cancellation.is_cancelled() && failure.is_none() {
            failure = Some("conversion_cancelled".to_owned());
            completion_deadline = Some(Instant::now() + CANCELLATION_COMPLETION_BOUND);
        }
        let current = now();
        if failure.is_none() && current >= deadline {
            cancellation.cancel();
            failure = Some("conversion_deadline_exceeded".to_owned());
            completion_deadline = Some(Instant::now() + CANCELLATION_COMPLETION_BOUND);
        }
        let wait_deadline = completion_deadline.unwrap_or(deadline);
        let wait_now = if failure.is_some() {
            Instant::now()
        } else {
            current
        };
        let remaining = wait_deadline.saturating_duration_since(wait_now);
        if remaining.is_zero() {
            return Err(failure.unwrap_or_else(|| "conversion_deadline_exceeded".into()));
        }
        let generation = cancellation.generation();
        if let Ok(value) = result.try_recv() {
            return failure.map_or(value, Err);
        }
        if cancellation.is_cancelled() && failure.is_none() {
            continue;
        }
        cancellation.wait_for_change(generation, remaining);
    }
}

const CANCELLATION_COMPLETION_BOUND: std::time::Duration = std::time::Duration::from_secs(2);

fn convert_inner(
    input: &Path,
    output_dir: &Path,
    plan: Vec<Step>,
    deadline: Instant,
    cancellation: ConversionCancellation,
) -> Result<String, String> {
    if cancellation.is_cancelled() {
        return Err("conversion_cancelled".into());
    }
    if matches!(plan.as_slice(), [Step::Identity]) {
        let metadata =
            fs::metadata(input).map_err(|error| format!("inspect text input: {error}"))?;
        if metadata.len() > MAX_CONVERTED_TEXT_BYTES as u64 {
            return Err("converted_text_oversize".into());
        }
        return fs::read_to_string(input).map_err(|error| format!("read text input: {error}"));
    }
    let mut current = input.to_path_buf();
    let mut transcript = None;
    for (index, step) in plan.into_iter().enumerate() {
        let Step::Convert { key, conversion } = step else {
            continue;
        };
        if Instant::now() >= deadline {
            return Err("conversion_deadline_exceeded".into());
        }
        if cancellation.is_cancelled() {
            return Err("conversion_cancelled".into());
        }
        let output = key
            .convert(&ConvertRequest {
                input_path: current.clone(),
                from: conversion.from,
                to: conversion.to,
                output_dir: output_dir.to_path_buf(),
                language: None,
                deadline,
                cancellation: cancellation.clone(),
            })
            .map_err(convert_reason)?;
        match output {
            ConvertOutput::Text(text) if conversion.to == MediaKind::Text => {
                if text.len() > MAX_CONVERTED_TEXT_BYTES {
                    return Err("converted_text_oversize".into());
                }
                transcript = Some(text);
            }
            ConvertOutput::Text(text) => {
                current = output_dir.join(format!("conversion-{index}.txt"));
                fs::write(&current, text)
                    .map_err(|error| format!("write conversion input: {error}"))?;
            }
            ConvertOutput::File(path) => {
                if !output_within(output_dir, &path) {
                    return Err("converter_output_outside_run_dir".into());
                }
                current = path;
                if conversion.to == MediaKind::Text {
                    let metadata = fs::metadata(&current)
                        .map_err(|error| format!("inspect converted text: {error}"))?;
                    if metadata.len() > MAX_CONVERTED_TEXT_BYTES as u64 {
                        return Err("converted_text_oversize".into());
                    }
                    transcript = Some(
                        fs::read_to_string(&current)
                            .map_err(|error| format!("read converted text: {error}"))?,
                    );
                }
            }
        }
    }
    if Instant::now() >= deadline {
        return Err("conversion_deadline_exceeded".into());
    }
    transcript.ok_or_else(|| "converter_returned_no_text".into())
}

fn convert_reason(error: ConvertError) -> String {
    match error {
        ConvertError::DeadlineExceeded => "conversion_deadline_exceeded".into(),
        ConvertError::Cancelled => "conversion_cancelled".into(),
        ConvertError::NoOutput => "converter_empty_output".into(),
        ConvertError::OutputOutsideDir(_) => "converter_output_outside_run_dir".into(),
        ConvertError::InvalidRequest(reason) | ConvertError::RequestFailed(reason) => {
            format!("converter_error: {reason}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeterm_plugin::converter::{Conversion, Converter};
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::Duration;

    struct FixedConverter {
        output: String,
    }

    impl Converter for FixedConverter {
        fn conversions(&self) -> &[Conversion] {
            static EDGES: [Conversion; 1] = [Conversion {
                from: MediaKind::Audio,
                to: MediaKind::Text,
            }];
            &EDGES
        }

        fn convert(&self, _: &ConvertRequest) -> Result<ConvertOutput, ConvertError> {
            Ok(ConvertOutput::Text(self.output.clone()))
        }
    }

    struct HeldConverter {
        started: mpsc::SyncSender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }

    struct ErrorConverter;

    struct CancellableConverter {
        started: mpsc::SyncSender<()>,
    }

    impl Converter for ErrorConverter {
        fn conversions(&self) -> &[Conversion] {
            static EDGES: [Conversion; 1] = [Conversion {
                from: MediaKind::Audio,
                to: MediaKind::Text,
            }];
            &EDGES
        }

        fn convert(&self, _: &ConvertRequest) -> Result<ConvertOutput, ConvertError> {
            Err(ConvertError::RequestFailed("fixture failure".into()))
        }
    }

    impl Converter for CancellableConverter {
        fn conversions(&self) -> &[Conversion] {
            static EDGES: [Conversion; 1] = [Conversion {
                from: MediaKind::Audio,
                to: MediaKind::Text,
            }];
            &EDGES
        }

        fn convert(&self, request: &ConvertRequest) -> Result<ConvertOutput, ConvertError> {
            self.started.send(()).map_err(|_| ConvertError::NoOutput)?;
            if request.cancellation.is_cancelled() {
                return Err(ConvertError::Cancelled);
            }
            let generation = request.cancellation.generation();
            if request.cancellation.is_cancelled() {
                return Err(ConvertError::Cancelled);
            }
            request.cancellation.wait_for_change(
                generation,
                request.deadline.saturating_duration_since(Instant::now()),
            );
            if request.cancellation.is_cancelled() {
                Err(ConvertError::Cancelled)
            } else {
                Err(ConvertError::DeadlineExceeded)
            }
        }
    }

    impl Converter for HeldConverter {
        fn conversions(&self) -> &[Conversion] {
            static EDGES: [Conversion; 1] = [Conversion {
                from: MediaKind::Audio,
                to: MediaKind::Text,
            }];
            &EDGES
        }

        fn convert(&self, _: &ConvertRequest) -> Result<ConvertOutput, ConvertError> {
            self.started.send(()).map_err(|_| ConvertError::NoOutput)?;
            self.release
                .lock()
                .map_err(|_| ConvertError::NoOutput)?
                .recv()
                .map_err(|_| ConvertError::NoOutput)?;
            Ok(ConvertOutput::Text("slow".into()))
        }
    }

    fn registry(
        converter: impl Converter + 'static,
    ) -> codeterm_plugin::converter::ConverterRegistry {
        let registry = codeterm_plugin::converter::ConverterRegistry::new();
        registry.register("capability", Arc::new(converter));
        registry
    }

    fn convert_test(
        registry: &codeterm_plugin::converter::ConverterRegistry,
        input: &Path,
        output_dir: &Path,
        deadline: Instant,
    ) -> Result<String, String> {
        convert(
            registry,
            input,
            output_dir,
            MediaKind::Audio,
            MediaKind::Text,
            deadline,
            Default::default(),
            None,
            Instant::now,
        )
    }

    #[test]
    fn blueprint_input_convert_uses_declared_converter_capability() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("voice.wav");
        fs::write(&input, b"audio").unwrap();
        let registry = registry(FixedConverter {
            output: "The quick brown fox".into(),
        });
        assert_eq!(
            convert_test(
                &registry,
                &input,
                temp.path(),
                Instant::now() + Duration::from_secs(5)
            )
            .unwrap(),
            "The quick brown fox"
        );
    }

    #[test]
    fn blueprint_input_convert_refuses_missing_converter_and_expired_deadline() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("voice.wav");
        fs::write(&input, b"audio").unwrap();
        let empty = codeterm_plugin::converter::ConverterRegistry::new();
        assert!(convert_test(
            &empty,
            &input,
            temp.path(),
            Instant::now() + Duration::from_secs(5)
        )
        .unwrap_err()
        .starts_with("no converter"));
        let available = registry(FixedConverter {
            output: "ignored".into(),
        });
        assert_eq!(
            convert_test(
                &available,
                &input,
                temp.path(),
                Instant::now() - Duration::from_millis(1)
            )
            .unwrap_err(),
            "conversion_deadline_exceeded"
        );
        let failing = registry(ErrorConverter);
        assert_eq!(
            convert_test(
                &failing,
                &input,
                temp.path(),
                Instant::now() + Duration::from_secs(5)
            )
            .unwrap_err(),
            "converter_error: fixture failure"
        );
    }

    #[test]
    fn blueprint_input_slow_converter_does_not_block_another_conversion() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("voice.wav");
        fs::write(&input, b"audio").unwrap();
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::channel();
        let slow = Arc::new(registry(HeldConverter {
            started: started_tx,
            release: Mutex::new(release_rx),
        }));
        let slow_input = input.clone();
        let slow_output = temp.path().to_path_buf();
        let slow_thread = std::thread::spawn(move || {
            convert_test(
                &slow,
                &slow_input,
                &slow_output,
                Instant::now() + Duration::from_secs(5),
            )
        });
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let fast = Arc::new(registry(FixedConverter {
            output: "fast".into(),
        }));
        let fast_input = input.clone();
        let fast_output = temp.path().to_path_buf();
        let (done_tx, done_rx) = mpsc::channel();
        let fast_thread = std::thread::spawn(move || {
            let result = convert_test(
                &fast,
                &fast_input,
                &fast_output,
                Instant::now() + Duration::from_secs(5),
            );
            let _ = done_tx.send(result);
        });
        assert_eq!(
            done_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .unwrap(),
            "fast"
        );
        release_tx.send(()).unwrap();
        assert_eq!(slow_thread.join().unwrap().unwrap(), "slow");
        fast_thread.join().unwrap();
    }

    #[test]
    fn blueprint_input_active_conversion_observes_stop_cancellation() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("voice.wav");
        fs::write(&input, b"audio").unwrap();
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let registry = Arc::new(registry(CancellableConverter {
            started: started_tx,
        }));
        let cancellation = ConversionCancellation::default();
        let worker_cancellation = cancellation.clone();
        let worker_input = input.clone();
        let worker_output = temp.path().to_path_buf();
        let worker = std::thread::spawn(move || {
            convert(
                &registry,
                &worker_input,
                &worker_output,
                MediaKind::Audio,
                MediaKind::Text,
                Instant::now() + Duration::from_secs(30),
                worker_cancellation,
                None,
                Instant::now,
            )
        });
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        cancellation.cancel();
        assert_eq!(worker.join().unwrap().unwrap_err(), "conversion_cancelled");
    }

    #[test]
    fn blueprint_input_conversion_deadline_uses_injected_clock_and_completion_event() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("voice.wav");
        fs::write(&input, b"audio").unwrap();
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let registry = registry(CancellableConverter {
            started: started_tx,
        });
        let deadline = Instant::now() + Duration::from_secs(30);
        let fake_now = move || deadline + Duration::from_secs(1);
        assert_eq!(
            convert(
                &registry,
                &input,
                temp.path(),
                MediaKind::Audio,
                MediaKind::Text,
                deadline,
                ConversionCancellation::default(),
                None,
                fake_now,
            )
            .unwrap_err(),
            "conversion_deadline_exceeded"
        );
        let _ = started_rx.try_recv();
    }
}
