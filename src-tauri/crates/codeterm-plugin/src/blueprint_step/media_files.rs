use super::{PortKind, RunMediaResolver};
use crate::media_delivery::{open_owned_media, OpenedMedia, RunMediaLookupContext};
use crate::view_host::{has_view_call_context, ViewRegistryHandle};
use codeterm_decision::physical::{self, RequestScope};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::time::{Duration, Instant};

pub struct RunMediaFiles {
    pub registry: ViewRegistryHandle,
    pub runtime: tokio::runtime::Handle,
}

impl RunMediaFiles {
    pub fn open(
        &self,
        context: &RunMediaLookupContext,
        media_id: &str,
        kind: PortKind,
        expected: Option<&str>,
    ) -> Result<OpenedMedia, String> {
        self.open_verified(context, media_id, kind, expected)
            .map(|(opened, _)| opened)
    }

    fn open_verified(
        &self,
        context: &RunMediaLookupContext,
        media_id: &str,
        kind: PortKind,
        expected: Option<&str>,
    ) -> Result<(OpenedMedia, String), String> {
        if !context.valid_for(&context.plugin)
            || has_view_call_context()
            || media_id.is_empty()
            || media_id.len() > 256
        {
            return Err("durable media lookup context refused".into());
        }
        let deadline = physical::deadline().map_or_else(
            || Instant::now() + Duration::from_secs(5),
            |outer| outer.min(Instant::now() + Duration::from_secs(5)),
        );
        let _scope = RequestScope::observation_only(deadline);
        let epoch = self
            .registry
            .registration_id(&context.plugin)
            .ok_or("durable media producer unavailable")?;
        let provider = self
            .registry
            .get(&context.plugin)
            .ok_or("durable media producer unavailable")?;
        let descriptor = provider
            .media_delivery()
            .ok_or("durable media capability unavailable")?;
        let source = self.runtime.block_on(async {
            tokio::time::timeout_at(deadline.into(), provider.run_media_file(media_id, context))
                .await
                .map_err(|_| "durable media lookup deadline")?
                .map_err(|_| "durable media source unavailable")
        })?;
        self.ensure_epoch(&context.plugin, &epoch, deadline)?;
        let mut opened = open_owned_media(&context.plugin, &source, &descriptor)
            .map_err(|_| "durable media source refused")?;
        let prefix = match kind {
            PortKind::Image => "image/",
            PortKind::Audio => "audio/",
            PortKind::Video => "video/",
            _ => return Err("durable media type unavailable".into()),
        };
        if !opened.content_type.starts_with(prefix) {
            return Err("durable media type changed".into());
        }
        let fingerprint = hash_opened(&mut opened, deadline)?;
        if expected.is_some_and(|expected| expected != fingerprint) {
            return Err("durable media bytes changed".into());
        }
        self.ensure_epoch(&context.plugin, &epoch, deadline)?;
        Ok((opened, fingerprint))
    }

    fn ensure_epoch(&self, plugin: &str, epoch: &str, deadline: Instant) -> Result<(), String> {
        if Instant::now() >= deadline
            || self.registry.registration_id(plugin).as_deref() != Some(epoch)
        {
            return Err("durable media producer changed or deadline reached".into());
        }
        Ok(())
    }
}

impl RunMediaResolver for RunMediaFiles {
    fn fingerprint(
        &self,
        context: &RunMediaLookupContext,
        media_id: &str,
        kind: PortKind,
    ) -> Result<String, String> {
        self.open_verified(context, media_id, kind, None)
            .map(|(_, fingerprint)| fingerprint)
    }
}

fn hash_opened(opened: &mut OpenedMedia, deadline: Instant) -> Result<String, String> {
    let mut hash = Sha256::new();
    let mut remaining = opened.size;
    let mut buffer = [0u8; 64 * 1024];
    while remaining > 0 {
        if Instant::now() >= deadline {
            return Err("durable media read deadline".into());
        }
        let bound = remaining.min(buffer.len() as u64) as usize;
        let count = opened
            .file
            .read(&mut buffer[..bound])
            .map_err(|_| "durable media read failed")?;
        if count == 0 {
            return Err("durable media source truncated".into());
        }
        hash.update(&buffer[..count]);
        remaining -= count as u64;
    }
    opened
        .file
        .seek(SeekFrom::Start(0))
        .map_err(|_| "durable media rewind failed")?;
    Ok(format!("{:x}", hash.finalize()))
}
