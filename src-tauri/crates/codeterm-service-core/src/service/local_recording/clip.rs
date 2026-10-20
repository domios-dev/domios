pub use codeterm_surface::local_recording::clip::CaptureClipDescriptor;
pub(crate) use codeterm_surface::local_recording::clip::CaptureClips;

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
