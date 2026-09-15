//! Host transcription of a receiver's audio part and the labelled rendering of both transcripts.

use std::path::Path;

use super::payload::AudioPart;
use super::{sanitize_event_text, MAX_DELIVERY_TEXT_BYTES};
use crate::transcribe::TranscribeRegistry;

pub(crate) const HOST_PRIMARY_LABEL: &str = "Transcript (CodeTerm):";
pub(crate) const SENDER_PRIMARY_LABEL: &str = "Transcript (sender):";
pub(crate) const HOST_COMPARISON_LABEL: &str = "For comparison, CodeTerm transcript:";
pub(crate) const SENDER_COMPARISON_LABEL: &str = "For comparison, sender transcript:";
pub(crate) const HOST_FAILURE_LABEL: &str = "CodeTerm transcription failed:";
pub(crate) const AUDIO_FILE_LABEL: &str = "Audio file:";

/// Lowercase tokens speech recognizers emit in place of words they could not decode.
const RECOGNIZER_FAILURE_MARKERS: &[&str] = &[
    "<unk>",
    "[blank_audio]",
    "[inaudible]",
    "(inaudible)",
    "[silence]",
    "(silence)",
    "[music]",
    "[no speech]",
];
const MIN_COMPARED_WORDS: usize = 8;
const SHORT_TRANSCRIPT_RATIO: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HostOutcome {
    Transcribed(String),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostAudio {
    pub path: Option<String>,
    pub outcome: HostOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Primary {
    Host,
    Sender,
}

fn clip(text: &str) -> String {
    let mut end = text.len().min(MAX_DELIVERY_TEXT_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// Stores the part with the normal upload rules, then runs the same voice stage as bridge voice notes.
pub(crate) async fn transcribe(
    transcribers: &TranscribeRegistry,
    active_id: &str,
    uploads_dir: &Path,
    audio: AudioPart,
) -> HostAudio {
    let stored =
        crate::api::web_routes::store_upload(uploads_dir, &audio.file_name, &audio.bytes).await;
    let path = match stored {
        Ok(path) => path,
        Err(error) => {
            log::warn!("[pebble-webhook] audio part could not be stored: {error}");
            return HostAudio {
                path: None,
                outcome: HostOutcome::Failed("the audio could not be stored on this host".into()),
            };
        }
    };
    let outcome = match crate::chatpipeline::run_voice_stage(transcribers, active_id, &path, "")
        .await
    {
        Ok(transcript) => HostOutcome::Transcribed(clip(&sanitize_event_text(&transcript.text))),
        Err(error) => {
            log::warn!("[pebble-webhook] host transcription failed: {error}");
            HostOutcome::Failed(crate::bridge::voice_transcribe::untranscribed_note(&error))
        }
    };
    HostAudio {
        path: Some(path.to_string_lossy().to_string()),
        outcome,
    }
}

/// Empty, recognizer failure markers, or under a quarter of the other transcript's words once it has eight.
pub(crate) fn degraded(candidate: &str, other: Option<&str>) -> bool {
    let words = candidate.split_whitespace().count();
    let lower = candidate.to_lowercase();
    if words == 0 || RECOGNIZER_FAILURE_MARKERS.iter().any(|m| lower.contains(m)) {
        return true;
    }
    other
        .map(|other| other.split_whitespace().count())
        .is_some_and(|other_words| {
            other_words >= MIN_COMPARED_WORDS && words * SHORT_TRANSCRIPT_RATIO < other_words
        })
}

/// The host transcript wins unless it is degraded while the sender's is not.
pub(crate) fn choose_primary(host: &str, sender: Option<&str>) -> Primary {
    match sender {
        Some(sender) if degraded(host, Some(sender)) && !degraded(sender, Some(host)) => {
            Primary::Sender
        }
        _ => Primary::Host,
    }
}

pub(crate) fn render(text: &str, sender: Option<&str>, host: Option<&HostAudio>) -> String {
    let mut lines = vec![text.to_string()];
    let Some(host) = host else {
        if let Some(sender) = sender {
            lines.push(format!("{SENDER_PRIMARY_LABEL} {sender}"));
        }
        return lines.join("\n");
    };
    match &host.outcome {
        HostOutcome::Transcribed(transcript) => match choose_primary(transcript, sender) {
            Primary::Host => {
                lines.push(format!("{HOST_PRIMARY_LABEL} {transcript}"));
                if let Some(sender) = sender {
                    lines.push(format!("{SENDER_COMPARISON_LABEL} {sender}"));
                }
            }
            Primary::Sender => {
                if let Some(sender) = sender {
                    lines.push(format!("{SENDER_PRIMARY_LABEL} {sender}"));
                }
                lines.push(format!("{HOST_COMPARISON_LABEL} {transcript}"));
            }
        },
        HostOutcome::Failed(note) => {
            if let Some(sender) = sender {
                lines.push(format!("{SENDER_PRIMARY_LABEL} {sender}"));
            }
            lines.push(format!("{HOST_FAILURE_LABEL} {note}"));
        }
    }
    if let Some(path) = &host.path {
        lines.push(format!("{AUDIO_FILE_LABEL} {path}"));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests;
