use std::sync::{mpsc, Arc};
use std::time::Duration;

use super::{index, ActiveEmbedder, EmbedError, EmbedderIdentity};
use crate::MemManager;

/// Resume passes per run; each pass embeds only units changed since the last.
const PASSES: usize = 8;
/// Consecutive unavailable units that mean the provider itself is down.
const OUTAGE_STREAK: usize = 5;
const PASSAGE_BATCH: usize = 8;

pub(crate) fn watch_progress(
    mut receive: impl FnMut(Duration) -> Result<(), String>,
    current: impl Fn() -> bool,
    fail: impl Fn(String),
) {
    loop {
        if let Err(error) = receive(Duration::from_secs(180)) {
            fail(format!("re-embed worker stopped advancing: {error}"));
            return;
        }
        if !current() {
            return;
        }
    }
}

use crate::backend::engines::ShadowUnit as Unit;

fn embed_unit(
    target: &ActiveEmbedder,
    identity: &EmbedderIdentity,
    unit: &Unit,
    progress: impl Fn(usize, usize) -> bool,
) -> Result<Vec<u8>, EmbedError> {
    let texts = unit
        .texts
        .iter()
        .map(|(_, _, text)| text.clone())
        .collect::<Vec<_>>();
    let mut blob = Vec::with_capacity(texts.len() * identity.dimension * 4);
    for (batch_index, batch) in texts.chunks(PASSAGE_BATCH).enumerate() {
        if batch_index == 0 && !progress(0, texts.len()) {
            return Err(EmbedError::Refused("re-embed cancelled".into()));
        }
        for result in target.passages(batch) {
            let vector = result?;
            if vector.identity != *identity {
                return Err(EmbedError::Refused("mixed-model shadow vector".into()));
            }
            super::provider::validate_vector(&vector.vector, identity.dimension)?;
            blob.extend(index::encode(&vector.vector));
        }
        if !progress(
            (batch_index * PASSAGE_BATCH + batch.len()).min(texts.len()),
            texts.len(),
        ) {
            return Err(EmbedError::Refused("re-embed cancelled".into()));
        }
    }
    Ok(blob)
}

impl MemManager {
    fn embed_shadow_unit(
        &self,
        generation: u64,
        target: &ActiveEmbedder,
        identity: &EmbedderIdentity,
        unit: &Unit,
        changed: &mpsc::Sender<()>,
    ) -> Result<Vec<u8>, EmbedError> {
        embed_unit(target, identity, unit, |done, total| {
            let current = if total > PASSAGE_BATCH {
                self.reembed
                    .item_progress(generation, &unit.id, done, total, &self.embedder)
            } else {
                self.reembed.is_current(generation)
            };
            let _ = changed.send(());
            current
        })
    }

    #[cfg(test)]
    pub(super) fn prepare_shadow(&self, identity: &EmbedderIdentity) -> Result<(), String> {
        self.store
            .prepare_engine_switch(&Self::write_context(), identity, 0, true)
            .map(|_| ())
    }

    pub(super) fn run_shadow(
        self: &Arc<Self>,
        generation: u64,
        target: Arc<ActiveEmbedder>,
        changed: &mpsc::Sender<()>,
    ) -> Result<(), String> {
        let identity = target.identity();
        let stamp = serde_json::to_string(&identity).map_err(|error| error.to_string())?;
        let mut docs = None;
        for pass in 0..PASSES {
            let _ = changed.send(());
            let (stale, total) = self.store.shadow_stale(&stamp)?;
            let stale_count = stale.len() as u64;
            if pass == 0 {
                self.reembed
                    .resume(generation, total - stale_count, 0, &self.embedder);
            } else {
                self.reembed
                    .resume(generation, 0, stale_count, &self.embedder);
            }
            let _ = changed.send(());
            let mut streak = 0;
            let mut unavailable = Vec::new();
            for unit in &stale {
                if !self.reembed.is_current(generation) {
                    return Err("re-embed cancelled".into());
                }
                let (vectors, error) =
                    match self.embed_shadow_unit(generation, &target, &identity, unit, changed) {
                        Ok(blob) => {
                            streak = 0;
                            (Some(blob), None)
                        }
                        Err(EmbedError::NotBuilt(reason)) => return Err(reason),
                        Err(EmbedError::Unavailable(reason)) => {
                            streak += 1;
                            if streak >= OUTAGE_STREAK {
                                return Err(format!(
                                    "provider unavailable for {streak} consecutive items: {reason}"
                                ));
                            }
                            unavailable.push(unit);
                            (None, Some(format!("{}: {reason}", unit.id)))
                        }
                        Err(EmbedError::Refused(reason)) => {
                            (None, Some(format!("{}: {reason}", unit.id)))
                        }
                    };
                self.store.shadow_checkpoint(
                    &Self::write_context(),
                    &stamp,
                    unit,
                    vectors.as_deref(),
                    false,
                    &|| self.reembed.is_current(generation),
                )?;
                if !self
                    .reembed
                    .advance(generation, error.map_or(Ok(()), Err), &self.embedder)
                {
                    return Err("re-embed cancelled".into());
                }
                let _ = changed.send(());
            }
            for unit in unavailable {
                if !self.reembed.is_current(generation) {
                    return Err("re-embed cancelled".into());
                }
                if let Ok(blob) =
                    self.embed_shadow_unit(generation, &target, &identity, unit, changed)
                {
                    self.store.shadow_checkpoint(
                        &Self::write_context(),
                        &stamp,
                        unit,
                        Some(&blob),
                        true,
                        &|| self.reembed.is_current(generation),
                    )?;
                    self.reembed.recovered(generation);
                }
                let _ = changed.send(());
            }
            if docs.is_none() {
                if let Some(data_dir) = &self.docs_dir {
                    docs = Some(crate::docs_source::prepare_reembed(
                        data_dir,
                        &target,
                        self.docs_sections(),
                        || {
                            self.reembed.advance(generation, Ok(()), &self.embedder);
                            let _ = changed.send(());
                        },
                    )?);
                }
            }
            let _ = changed.send(());
            let published = self.store.publish_shadow(
                &Self::write_context(),
                &target,
                &self.embedder,
                &stamp,
                &|| self.reembed.claim(generation),
            )?;
            let _ = changed.send(());
            if published {
                if let Some(docs) = docs {
                    crate::docs_source::publish_reembed(docs, &self.embedder);
                }
                self.embedder
                    .emit(&super::EmbedderEvent::Switched(identity));
                self.reembed.finish(generation, &self.embedder);
                return Ok(());
            }
        }
        Err(format!(
            "memory kept changing across {PASSES} re-embed passes; switch again to resume"
        ))
    }

    /// Diffs the live units against the checkpoints inside one IMMEDIATE
    /// transaction; `false` means a unit changed and another pass is needed.
    pub(super) fn launch_shadow(
        self: &Arc<Self>,
        generation: u64,
        target: Arc<ActiveEmbedder>,
    ) -> Result<(), String> {
        let (changed, progress) = mpsc::channel();
        let manager = self.clone();
        std::thread::Builder::new()
            .name("mem-reembed".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    manager.run_shadow(generation, target, &changed)
                }));
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => manager.reembed.fail(generation, error, &manager.embedder),
                    Err(_) => manager.reembed.fail(
                        generation,
                        "re-embed worker panicked".into(),
                        &manager.embedder,
                    ),
                }
            })
            .map_err(|error| format!("start shadow worker: {error}"))?;
        let manager = self.clone();
        std::thread::Builder::new()
            .name("mem-reembed-watchdog".into())
            .spawn(move || {
                watch_progress(
                    |timeout| {
                        progress
                            .recv_timeout(timeout)
                            .map_err(|error| error.to_string())
                    },
                    || manager.reembed.is_current(generation),
                    |error| manager.reembed.fail(generation, error, &manager.embedder),
                );
            })
            .map_err(|error| format!("start re-embed watchdog: {error}"))?;
        Ok(())
    }
}
