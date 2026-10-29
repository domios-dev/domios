use super::*;
use crate::SecretString;
use std::future::{poll_fn, Future};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::Poll;
use tokio::sync::{oneshot, Semaphore};

struct GatedStore {
    started: parking_lot::Mutex<Option<oneshot::Sender<()>>>,
    release: Semaphore,
    initialized: AtomicBool,
    init_calls: AtomicUsize,
    get_calls: AtomicUsize,
    value: &'static str,
    fail_init: bool,
}

fn gated(value: &'static str) -> (Arc<GatedStore>, oneshot::Receiver<()>) {
    let (sender, receiver) = oneshot::channel();
    (
        Arc::new(GatedStore {
            started: parking_lot::Mutex::new(Some(sender)),
            release: Semaphore::new(0),
            initialized: AtomicBool::new(false),
            init_calls: AtomicUsize::new(0),
            get_calls: AtomicUsize::new(0),
            value,
            fail_init: false,
        }),
        receiver,
    )
}

#[async_trait]
impl SecretStore for GatedStore {
    fn id(&self) -> &'static str {
        "gated-vault"
    }

    async fn init(&self) -> Result<(), SecretError> {
        self.init_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(started) = self.started.lock().take() {
            let _ = started.send(());
        }
        self.release.acquire().await.unwrap().forget();
        if self.fail_init {
            return Err(SecretError::Locked);
        }
        self.initialized.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn status(&self) -> StoreStatus {
        if self.initialized.load(Ordering::SeqCst) {
            StoreStatus::Unlocked {
                user: None,
                transient: true,
                endpoint: None,
            }
        } else {
            StoreStatus::Locked { endpoint: None }
        }
    }

    async fn get(&self, name: &str) -> Result<SecretValue, SecretError> {
        self.get_calls.fetch_add(1, Ordering::SeqCst);
        if !self.initialized.load(Ordering::SeqCst) {
            return Err(SecretError::Locked);
        }
        Ok(SecretValue {
            item_id: "opaque-id".into(),
            name: name.into(),
            value: SecretString::from(self.value),
            notes: None,
        })
    }

    async fn unlock(&self, _: UnlockCredentials) -> Result<(), SecretError> {
        self.initialized.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn lock(&self) -> Result<(), SecretError> {
        Ok(())
    }

    async fn logout(&self) -> Result<(), SecretError> {
        Ok(())
    }

    async fn save(&self, _: SaveRequest) -> Result<SecretRef, SecretError> {
        Err(SecretError::NotFound)
    }

    async fn list(&self, _: ListFilter) -> Result<Vec<SecretRef>, SecretError> {
        Ok(Vec::new())
    }

    async fn search(&self, _: &str) -> Result<Vec<SecretRef>, SecretError> {
        Ok(Vec::new())
    }

    async fn delete(&self, _: &str) -> Result<(), SecretError> {
        Ok(())
    }
}

fn selected(registry: &Arc<crate::SecretRegistry>) -> Arc<dyn SecretStore> {
    crate::build_active_store(
        &codeterm_config::schema::SecretsConfig {
            backend: "gated".into(),
            ..Default::default()
        },
        registry,
    )
}

#[tokio::test]
async fn startup_and_early_consumers_share_one_cancellation_safe_init_flight() {
    let registry = Arc::new(crate::SecretRegistry::new());
    let (raw, started) = gated("selected-value");
    registry.register("gated", raw.clone());
    let warmup = selected(&registry);
    let task = tokio::spawn(async move { warmup.init().await });
    started.await.unwrap();

    let consumer = selected(&registry);
    let mut read = Box::pin(consumer.get("key"));
    assert!(poll_fn(|cx| Poll::Ready(read.as_mut().poll(cx)))
        .await
        .is_pending());
    assert_eq!(raw.get_calls.load(Ordering::SeqCst), 0);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    raw.release.add_permits(1);

    assert_eq!(read.await.unwrap().value.expose_secret(), "selected-value");
    consumer.init().await.unwrap();
    assert_eq!(raw.init_calls.load(Ordering::SeqCst), 1);
    assert_eq!(raw.get_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn reloaded_registration_waits_for_its_own_init_and_pins_each_consumer() {
    let registry = Arc::new(crate::SecretRegistry::new());
    let (old, old_started) = gated("old-generation");
    registry.register("gated", old.clone());
    let active = selected(&registry);
    let mut old_read = Box::pin(active.get("key"));
    assert!(poll_fn(|cx| Poll::Ready(old_read.as_mut().poll(cx)))
        .await
        .is_pending());
    old_started.await.unwrap();

    let (new, new_started) = gated("new-generation");
    registry.register("gated", new.clone());
    let mut new_read = Box::pin(active.get("key"));
    assert!(poll_fn(|cx| Poll::Ready(new_read.as_mut().poll(cx)))
        .await
        .is_pending());
    new_started.await.unwrap();
    old.release.add_permits(1);
    assert_eq!(old_read.await.unwrap().value.expose_secret(), "old-generation");
    assert!(poll_fn(|cx| Poll::Ready(new_read.as_mut().poll(cx)))
        .await
        .is_pending());
    assert_eq!(new.get_calls.load(Ordering::SeqCst), 0);

    new.release.add_permits(1);
    assert_eq!(new_read.await.unwrap().value.expose_secret(), "new-generation");
    assert_eq!(old.init_calls.load(Ordering::SeqCst), 1);
    assert_eq!(new.init_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failed_init_is_settled_and_does_not_block_explicit_unlock() {
    let (mut raw, started) = gated("unlocked-value");
    Arc::get_mut(&mut raw).unwrap().fail_init = true;
    let active = wrap(raw.clone());
    let warmup = active.clone();
    let task = tokio::spawn(async move { warmup.init().await });
    started.await.unwrap();
    raw.release.add_permits(1);
    assert!(matches!(task.await.unwrap(), Err(SecretError::Locked)));
    assert!(matches!(active.status().await, StoreStatus::Locked { .. }));
    active
        .unlock(UnlockCredentials::with_password(SecretString::from("inert")))
        .await
        .unwrap();
    assert_eq!(active.get("key").await.unwrap().value.expose_secret(), "unlocked-value");
    assert_eq!(raw.init_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn actual_js_registration_initializes_before_get_after_each_reload() {
    use codeterm_plugin::SecretBackendRegistrar;
    use codeterm_plugin::js_runtime::PluginVm;

    let registry = Arc::new(crate::SecretRegistry::new());
    let registrar = crate::js_backend::AppSecretBackendRegistrar::new(registry.clone());
    let source = r#"
        let initialized = false;
        module.exports.default = {
            secretInit() { initialized = true; return {ok: {}}; },
            secretGetItem(name) {
                return initialized
                    ? {ok: {item_id: 'id', name, value: 'initialized-generation'}}
                    : {error: {kind: 'locked'}};
            }
        };
    "#;
    for _ in 0..2 {
        let vm = Arc::new(PluginVm::from_source("gated", source.into()).unwrap());
        registrar.register_secret_backend("gated".into(), vm, false);
        assert_eq!(
            selected(&registry).get("key").await.unwrap().value.expose_secret(),
            "initialized-generation"
        );
    }
}
