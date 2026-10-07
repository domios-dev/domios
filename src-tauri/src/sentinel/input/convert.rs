use std::fs;
use std::path::Path;
use std::time::Instant;

use codeterm_plugin::converter::{
    output_within, ConvertError, ConvertOutput, ConvertRequest, Step,
};
use codeterm_types::sentinel_input::{MediaKind, MAX_CONVERTED_TEXT_BYTES};

pub(super) fn convert(
    registry: &codeterm_plugin::converter::ConverterRegistry,
    input: &Path,
    output_dir: &Path,
    from: MediaKind,
    to: MediaKind,
    deadline: Instant,
) -> Result<String, String> {
    if from == MediaKind::Text && to == MediaKind::Text {
        let metadata =
            fs::metadata(input).map_err(|error| format!("inspect text input: {error}"))?;
        if metadata.len() > MAX_CONVERTED_TEXT_BYTES as u64 {
            return Err("converted_text_oversize".into());
        }
        return fs::read_to_string(input).map_err(|error| format!("read text input: {error}"));
    }
    let plan = registry.plan(from, to).map_err(|error| error.to_string())?;
    let mut current = input.to_path_buf();
    let mut transcript = None;
    for (index, step) in plan.into_iter().enumerate() {
        let Step::Convert { key, conversion } = step else {
            continue;
        };
        if Instant::now() >= deadline {
            return Err("conversion_deadline_exceeded".into());
        }
        let output = key
            .convert(&ConvertRequest {
                input_path: current,
                from: conversion.from,
                to: conversion.to,
                output_dir: output_dir.to_path_buf(),
                language: None,
                deadline,
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

    #[test]
    fn blueprint_input_convert_uses_declared_converter_capability() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("voice.wav");
        fs::write(&input, b"audio").unwrap();
        let registry = registry(FixedConverter {
            output: "The quick brown fox".into(),
        });
        assert_eq!(
            convert(
                &registry,
                &input,
                temp.path(),
                MediaKind::Audio,
                MediaKind::Text,
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
        assert!(convert(
            &empty,
            &input,
            temp.path(),
            MediaKind::Audio,
            MediaKind::Text,
            Instant::now() + Duration::from_secs(5)
        )
        .unwrap_err()
        .starts_with("no converter"));
        let available = registry(FixedConverter {
            output: "ignored".into(),
        });
        assert_eq!(
            convert(
                &available,
                &input,
                temp.path(),
                MediaKind::Audio,
                MediaKind::Text,
                Instant::now() - Duration::from_millis(1)
            )
            .unwrap_err(),
            "conversion_deadline_exceeded"
        );
        let failing = registry(ErrorConverter);
        assert_eq!(
            convert(
                &failing,
                &input,
                temp.path(),
                MediaKind::Audio,
                MediaKind::Text,
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
            convert(
                &slow,
                &slow_input,
                &slow_output,
                MediaKind::Audio,
                MediaKind::Text,
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
            let result = convert(
                &fast,
                &fast_input,
                &fast_output,
                MediaKind::Audio,
                MediaKind::Text,
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
}
