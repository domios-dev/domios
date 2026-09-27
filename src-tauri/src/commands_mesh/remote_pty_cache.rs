use std::{collections::HashMap, future::Future, sync::LazyLock};

#[derive(Default)]
struct CacheState {
    values: HashMap<(String, String), String>,
    generations: HashMap<(String, String), u64>,
}

#[derive(Default)]
pub(super) struct RemotePtyIdCache(parking_lot::Mutex<CacheState>);

impl RemotePtyIdCache {
    pub(super) async fn get_or_resolve<F, Fut>(
        &self,
        key: (String, String),
        resolve: F,
    ) -> Result<String, String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<String, String>>,
    {
        let generation = {
            let cache = self.0.lock();
            if let Some(pty_id) = cache.values.get(&key) {
                return Ok(pty_id.clone());
            }
            cache.generations.get(&key).copied().unwrap_or_default()
        };
        let resolved = resolve().await?;
        let mut cache = self.0.lock();
        if cache.generations.get(&key).copied().unwrap_or_default() != generation {
            return Ok(resolved);
        }
        Ok(cache.values.entry(key).or_insert(resolved).clone())
    }

    pub(super) fn invalidate(&self, key: &(String, String)) {
        let mut cache = self.0.lock();
        cache.values.remove(key);
        let generation = cache.generations.entry(key.clone()).or_default();
        *generation = generation.wrapping_add(1);
    }
}

pub(super) static REMOTE_PTY_IDS: LazyLock<RemotePtyIdCache> =
    LazyLock::new(RemotePtyIdCache::default);

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn remote_pty_id_cache_skips_inspect_for_following_writes() {
        let cache = RemotePtyIdCache::default();
        let key = ("peer-a".to_string(), "tab-a".to_string());
        let mut lookups = 0;
        let first = cache
            .get_or_resolve(key.clone(), || async {
                lookups += 1;
                Ok::<_, String>("pty-a".to_string())
            })
            .await
            .unwrap();
        let second = cache
            .get_or_resolve(key.clone(), || async {
                lookups += 1;
                Ok::<_, String>("pty-b".to_string())
            })
            .await
            .unwrap();

        assert_eq!(first, "pty-a");
        assert_eq!(second, "pty-a");
        assert_eq!(lookups, 1);

        cache.invalidate(&key);
        let refreshed = cache
            .get_or_resolve(key, || async {
                lookups += 1;
                Ok::<_, String>("pty-b".to_string())
            })
            .await
            .unwrap();
        assert_eq!(refreshed, "pty-b");
        assert_eq!(lookups, 2);
    }
}
