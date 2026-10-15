use std::collections::HashMap;
use std::path::PathBuf;

use parking_lot::Mutex;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureClipDescriptor {
    pub(crate) handle: String,
    pub(crate) path: String,
}

#[derive(Default)]
pub(crate) struct CaptureClips {
    clips: Mutex<HashMap<String, OwnedCaptureClip>>,
}

struct OwnedCaptureClip {
    lease: String,
    plugin_id: String,
    generation: u64,
    path: Option<PathBuf>,
    stage: ClipStage,
    discard_requested: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ClipStage {
    Uploading,
    Stored,
    Transcribing,
    HandoffPossible,
}

impl CaptureClips {
    pub(crate) fn matches_generation(
        &self,
        handle: &str,
        lease: &str,
        plugin_id: &str,
        generation: u64,
    ) -> bool {
        self.clips.lock().get(handle).is_some_and(|clip| {
            clip.lease == lease && clip.plugin_id == plugin_id && clip.generation == generation
        })
    }

    pub(crate) fn reserve(&self, lease: &str, plugin_id: &str, generation: u64) -> String {
        let handle = uuid::Uuid::new_v4().to_string();
        self.clips.lock().insert(
            handle.clone(),
            OwnedCaptureClip {
                lease: lease.into(),
                plugin_id: plugin_id.into(),
                generation,
                path: None,
                stage: ClipStage::Uploading,
                discard_requested: false,
            },
        );
        handle
    }

    pub(crate) fn finish_upload(&self, handle: &str, path: PathBuf) -> Result<(), PathBuf> {
        let mut clips = self.clips.lock();
        let Some(can_store) = clips
            .get(handle)
            .map(|clip| clip.stage == ClipStage::Uploading && !clip.discard_requested)
        else {
            return Err(path);
        };
        if !can_store {
            clips.remove(handle);
            return Err(path);
        }
        let clip = clips.get_mut(handle).expect("clip was checked above");
        clip.path = Some(path);
        clip.stage = ClipStage::Stored;
        Ok(())
    }

    pub(crate) fn abandon_upload(&self, handle: &str) {
        self.clips.lock().remove(handle);
    }

    pub(crate) fn begin_transcription(&self, lease: &str, handle: &str) -> Result<PathBuf, String> {
        let mut clips = self.clips.lock();
        let clip = clips.get_mut(handle).ok_or("capture_clip_stale")?;
        if clip.lease != lease || clip.stage != ClipStage::Stored || clip.discard_requested {
            return Err("capture_clip_stale".into());
        }
        clip.stage = ClipStage::Transcribing;
        clip.path.clone().ok_or_else(|| "capture_clip_stale".into())
    }

    pub(crate) fn mark_handoff_possible(&self, lease: &str, handle: &str) -> Result<(), String> {
        let mut clips = self.clips.lock();
        let clip = clips.get_mut(handle).ok_or("capture_clip_stale")?;
        if clip.lease != lease || clip.stage != ClipStage::Transcribing {
            return Err("capture_clip_stale".into());
        }
        clip.stage = ClipStage::HandoffPossible;
        Ok(())
    }

    pub(crate) fn finish_pre_handoff(&self, lease: &str, handle: &str) -> Option<PathBuf> {
        let mut clips = self.clips.lock();
        if !clips.get(handle).is_some_and(|clip| clip.lease == lease) {
            return None;
        }
        clips.remove(handle).and_then(|clip| clip.path)
    }

    pub(crate) fn retain_after_handoff(&self, lease: &str, handle: &str) -> Result<(), String> {
        let mut clips = self.clips.lock();
        if !clips
            .get(handle)
            .is_some_and(|clip| clip.lease == lease && clip.stage == ClipStage::HandoffPossible)
        {
            return Err("capture_clip_stale".into());
        }
        clips.remove(handle);
        Ok(())
    }

    pub(crate) fn discard(&self, lease: &str, handle: &str) -> Option<PathBuf> {
        let mut clips = self.clips.lock();
        let (clip_lease, stage) = clips
            .get(handle)
            .map(|clip| (clip.lease.clone(), clip.stage))?;
        if clip_lease != lease {
            return None;
        }
        match stage {
            ClipStage::Uploading => {
                if let Some(clip) = clips.get_mut(handle) {
                    clip.discard_requested = true;
                }
                None
            }
            ClipStage::Stored => clips.remove(handle).and_then(|clip| clip.path),
            ClipStage::Transcribing | ClipStage::HandoffPossible => None,
        }
    }

    pub(crate) fn retire_lease(&self, lease: &str) -> Vec<PathBuf> {
        self.retire_matching(|clip| clip.lease == lease)
    }

    pub(crate) fn invalidate_generation(&self, plugin_id: &str, generation: u64) -> Vec<PathBuf> {
        self.retire_matching(|clip| clip.plugin_id == plugin_id && clip.generation == generation)
    }

    fn retire_matching(&self, matches: impl Fn(&OwnedCaptureClip) -> bool) -> Vec<PathBuf> {
        let mut clips = self.clips.lock();
        let handles = clips
            .iter()
            .filter_map(|(handle, clip)| matches(clip).then_some(handle.clone()))
            .collect::<Vec<_>>();
        let mut paths = Vec::new();
        for handle in handles {
            let Some(stage) = clips.get(&handle).map(|clip| clip.stage) else {
                continue;
            };
            match stage {
                ClipStage::Uploading | ClipStage::Transcribing => {
                    if let Some(clip) = clips.get_mut(&handle) {
                        clip.discard_requested = true;
                    }
                }
                ClipStage::Stored => {
                    if let Some(clip) = clips.remove(&handle) {
                        paths.extend(clip.path);
                    }
                }
                ClipStage::HandoffPossible => {}
            }
        }
        paths
    }

    pub fn cleanup(paths: Vec<PathBuf>) {
        for path in paths {
            if let Err(error) = std::fs::remove_file(&path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    log::debug!("[local-recording] owned clip cleanup failed: {error}");
                }
            }
        }
    }
}

impl crate::service::ServiceContext {
    pub async fn local_recording_save_clip(
        &self,
        lease: &str,
        data: String,
        extension: String,
    ) -> Result<CaptureClipDescriptor, String> {
        if !matches!(extension.as_str(), "webm" | "ogg" | "m4a") {
            return Err("capture_clip_extension_invalid".into());
        }
        let handle = self.local_recordings.reserve_clip(lease)?;
        let save = match tokio::task::spawn_blocking(move || {
            use base64::Engine as _;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&data)
                .map_err(|error| format!("capture_clip_base64_invalid: {error}"))?;
            if bytes.is_empty() || bytes.len() as u64 > crate::attachments::DEFAULT_MAX_BYTES {
                return Err("capture_clip_size_invalid".into());
            }
            crate::attachments::save_with_uuid(
                "chat",
                &extension,
                &bytes,
                crate::attachments::EvictionPolicy::MaxBytes(crate::attachments::DEFAULT_MAX_BYTES),
            )
        })
        .await
        {
            Ok(save) => save,
            Err(error) => {
                self.local_recordings.abandon_clip_upload(&handle);
                return Err(format!("capture_clip_worker_failed: {error}"));
            }
        };
        let path = match save {
            Ok(path) => path,
            Err(error) => {
                self.local_recordings.abandon_clip_upload(&handle);
                return Err(error);
            }
        };
        let path_text = path.to_string_lossy().into_owned();
        self.local_recordings
            .finish_clip_upload(lease, &handle, path)?;
        Ok(CaptureClipDescriptor {
            handle,
            path: path_text,
        })
    }

    pub fn local_recording_discard_clip(&self, lease: &str, handle: &str) {
        self.local_recordings.discard_clip(lease, handle);
    }
}

#[cfg(test)]
mod tests {
    use super::CaptureClips;
    use std::fs;

    fn owned_file(label: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("codeterm-lark-{label}-{}", uuid::Uuid::new_v4()));
        fs::write(&path, b"capture clip").unwrap();
        path
    }

    #[test]
    fn lark_recording_contract_late_upload_after_cancel_cleans_only_owned_clip() {
        let clips = CaptureClips::default();
        let handle = clips.reserve("lease-a", "lark", 7);
        let unrelated = owned_file("unrelated");
        assert!(clips.discard("lease-foreign", &handle).is_none());
        assert!(clips.retire_lease("lease-a").is_empty());
        let path = owned_file("late-upload");
        assert!(clips.finish_upload(&handle, path.clone()).is_err());
        CaptureClips::cleanup(vec![path.clone()]);
        assert!(!path.exists());
        assert!(unrelated.exists());
        let _ = fs::remove_file(unrelated);
    }

    #[test]
    fn lark_recording_contract_pre_handoff_failure_deletes_and_success_retains() {
        let clips = CaptureClips::default();
        let failed = clips.reserve("lease-fail", "lark", 8);
        let failed_path = owned_file("transcription-failure");
        clips.finish_upload(&failed, failed_path.clone()).unwrap();
        assert_eq!(
            clips.begin_transcription("lease-fail", &failed).unwrap(),
            failed_path
        );
        assert_eq!(
            clips.finish_pre_handoff("lease-fail", &failed),
            Some(failed_path.clone())
        );
        assert_eq!(clips.finish_pre_handoff("lease-fail", &failed), None);
        CaptureClips::cleanup(vec![failed_path.clone()]);
        assert!(!failed_path.exists());

        let cancelled = clips.reserve("lease-cancel", "lark", 9);
        let cancelled_path = owned_file("cancelled-transcription");
        clips
            .finish_upload(&cancelled, cancelled_path.clone())
            .unwrap();
        clips
            .begin_transcription("lease-cancel", &cancelled)
            .unwrap();
        assert!(clips.retire_lease("lease-cancel").is_empty());
        assert_eq!(
            clips.finish_pre_handoff("lease-cancel", &cancelled),
            Some(cancelled_path.clone())
        );
        CaptureClips::cleanup(vec![cancelled_path.clone()]);
        assert!(!cancelled_path.exists());

        let retained = clips.reserve("lease-ok", "lark", 10);
        let retained_path = owned_file("retained");
        clips
            .finish_upload(&retained, retained_path.clone())
            .unwrap();
        clips.begin_transcription("lease-ok", &retained).unwrap();
        clips.mark_handoff_possible("lease-ok", &retained).unwrap();
        clips.retire_lease("lease-ok");
        clips.retain_after_handoff("lease-ok", &retained).unwrap();
        assert!(retained_path.exists());
        let _ = fs::remove_file(retained_path);
    }

    #[test]
    fn lark_recording_contract_stale_generation_never_retires_new_clip() {
        let clips = CaptureClips::default();
        let current = clips.reserve("lease-current", "lark", 10);
        let path = owned_file("current-generation");
        clips.finish_upload(&current, path.clone()).unwrap();
        assert!(clips.invalidate_generation("lark", 9).is_empty());
        assert!(clips.begin_transcription("lease-current", &current).is_ok());
        assert!(path.exists());
        let _ = fs::remove_file(path);
    }
}
