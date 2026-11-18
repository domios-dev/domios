use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(crate) mod cache;
mod isolation;
mod shared_reads;
pub mod storage;
pub use shared_reads::{PinnedSharedReadGrant, SharedReadGrant};

#[derive(Clone)]
pub struct InvocationContext {
    provider: String,
    account: String,
    environment: BTreeMap<String, Option<String>>,
    read_roots: Vec<PathBuf>,
    protected_roots: Vec<PathBuf>,
    shared_reads: Vec<PinnedSharedReadGrant>,
    shared_exclusions: Vec<PathBuf>,
    pinned_read_roots: Vec<PathBuf>,
    isolation: isolation::Isolation,
    cache_key: String,
    credential_secret_scope: Option<String>,
}

impl std::fmt::Debug for InvocationContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InvocationContext")
            .field("provider", &self.provider)
            .field("account", &self.account)
            .finish_non_exhaustive()
    }
}

impl InvocationContext {
    pub fn new(
        provider: String,
        account: String,
        environment: BTreeMap<String, Option<String>>,
        read_roots: Vec<PathBuf>,
        protected_roots: Vec<PathBuf>,
    ) -> Result<Arc<Self>, String> {
        crate::plugin_id::validate(&provider)?;
        crate::plugin_id::validate(&account)?;
        if environment.iter().any(|(name, value)| {
            name.is_empty()
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                || !name
                    .as_bytes()
                    .first()
                    .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
                || value
                    .as_ref()
                    .is_some_and(|value| value.is_empty() || value.contains('\0'))
        }) || read_roots
            .iter()
            .chain(&protected_roots)
            .any(|root| !root.is_absolute())
        {
            return Err("invalid provider account invocation context".into());
        }
        let pinned_read_roots = read_roots
            .iter()
            .map(|root| {
                codeterm_util::project_roots::canonical_target(root)
                    .ok_or("Selected account read root cannot be resolved")
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut context = Self {
            provider,
            account,
            environment,
            read_roots,
            protected_roots,
            shared_reads: Vec::new(),
            shared_exclusions: Vec::new(),
            pinned_read_roots,
            isolation: Default::default(),
            cache_key: String::new(),
            credential_secret_scope: None,
        };
        context.cache_key = context.identity()?;
        Ok(Arc::new(context))
    }

    fn identity(&self) -> Result<String, String> {
        use sha2::{Digest, Sha256};
        let identity = serde_json::to_vec(&(
            &self.provider,
            &self.account,
            &self.environment,
            &self.read_roots,
            &self.protected_roots,
            &self.shared_reads,
            &self.shared_exclusions,
            &self.pinned_read_roots,
            &self.credential_secret_scope,
            &self.isolation.identities(),
            &self.isolation.roots(),
        ))
        .map_err(|error| error.to_string())?;
        Ok(format!("{:x}", Sha256::digest(identity)))
    }

    pub fn with_shared_reads(
        self: Arc<Self>,
        grants: Vec<SharedReadGrant>,
    ) -> Result<Arc<Self>, String> {
        if grants.len() > 64 {
            return Err("shared storage authority limit".into());
        }
        let pinned = grants
            .into_iter()
            .map(|grant| {
                grant.pin_with_identities(&self.shared_exclusions, &self.isolation.identities())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut context = (*self).clone();
        context.shared_reads = pinned;
        context.cache_key = context.identity()?;
        Ok(Arc::new(context))
    }

    pub fn with_shared_exclusions(
        self: Arc<Self>,
        exclusions: Vec<PathBuf>,
    ) -> Result<Arc<Self>, String> {
        if exclusions.len() > 1024
            || exclusions.iter().any(|path| !path.is_absolute())
            || !self.shared_reads.is_empty()
        {
            return Err("Invalid isolated storage exclusions".into());
        }
        let mut context = (*self).clone();
        let identity_paths = exclusions
            .iter()
            .filter(|path| !path.is_dir())
            .cloned()
            .collect::<Vec<_>>();
        context
            .isolation
            .extend(&identity_paths, &context.pinned_read_roots)?;
        for exclusion in exclusions {
            let pinned = codeterm_util::project_roots::canonical_target(&exclusion)
                .ok_or("Isolated storage target cannot be resolved")?;
            context.shared_exclusions.push(exclusion);
            context.shared_exclusions.push(pinned);
        }
        context.cache_key = context.identity()?;
        Ok(Arc::new(context))
    }

    pub fn with_isolated_storage_paths(
        self: Arc<Self>,
        paths: &[PathBuf],
    ) -> Result<Arc<Self>, String> {
        if !self.shared_reads.is_empty() || paths.iter().any(|path| !path.is_absolute()) {
            return Err("Invalid isolated storage paths".into());
        }
        let mut context = (*self).clone();
        context
            .isolation
            .extend(paths, &context.pinned_read_roots)?;
        context.cache_key = context.identity()?;
        Ok(Arc::new(context))
    }

    pub fn from_capability(
        provider: String,
        account: String,
        capability: &storage::AccountCapability,
        mut environment: BTreeMap<String, Option<String>>,
        read_roots: Vec<PathBuf>,
        protected_roots: Vec<PathBuf>,
    ) -> Result<Arc<Self>, String> {
        capability.validate()?;
        let home = environment
            .get(&capability.home_environment)
            .and_then(|value| value.as_ref())
            .filter(|value| Path::new(value).is_absolute())
            .ok_or("selected provider account home is required")?;
        if !read_roots.iter().any(|root| root == Path::new(home)) {
            return Err("selected provider account home must be an authorized read root".into());
        }
        let root = codeterm_util::project_roots::canonical_target(Path::new(home))
            .ok_or("Selected credential home cannot be resolved")?;
        let root = root
            .to_str()
            .ok_or("Selected credential home must be Unicode")?;
        let root = if cfg!(windows) {
            codeterm_paths::strip_verbatim_prefix(root).replace('\\', "/")
        } else {
            root.to_owned()
        };
        use sha2::{Digest, Sha256};
        let credential_secret_scope = format!("{:x}", Sha256::digest(root.as_bytes()));
        for name in &capability.excluded_environment {
            environment.entry(name.clone()).or_insert(None);
        }
        for name in &capability.secret_environment {
            environment.insert(name.clone(), None);
        }
        let mut context =
            (*Self::new(provider, account, environment, read_roots, protected_roots)?).clone();
        context.shared_exclusions = context
            .protected_roots
            .iter()
            .chain(&context.read_roots)
            .flat_map(|root| {
                capability
                    .isolated_paths
                    .iter()
                    .map(move |path| root.join(path))
            })
            .collect();
        let pinned_exclusions = context
            .shared_exclusions
            .iter()
            .map(|path| {
                codeterm_util::project_roots::canonical_target(path)
                    .ok_or("Isolated storage target cannot be resolved")
            })
            .collect::<Result<Vec<_>, _>>()?;
        context.shared_exclusions.extend(pinned_exclusions);
        context
            .isolation
            .extend(&context.shared_exclusions, &context.pinned_read_roots)?;
        context.credential_secret_scope = Some(credential_secret_scope);
        context.cache_key = context.identity()?;
        Ok(Arc::new(context))
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }
    pub fn account(&self) -> &str {
        &self.account
    }
    pub fn cache_key(&self) -> &str {
        &self.cache_key
    }

    pub fn credential_secret_scope(&self) -> Option<&str> {
        self.credential_secret_scope.as_deref()
    }

    pub fn credential_secret_prefix(&self, provider: &str) -> Option<String> {
        (self.provider() == provider).then_some(())?;
        Some(format!("account:{}:", self.credential_secret_scope()?))
    }

    pub fn environment_entries(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_deref()))
    }

    pub fn apply_environment(&self, env: &mut std::collections::HashMap<String, String>) {
        for (key, value) in &self.environment {
            env.retain(|existing, _| !environment_name_matches(existing, key));
            match value {
                Some(value) => {
                    env.insert(key.clone(), value.clone());
                }
                None => {
                    env.remove(key);
                }
            }
        }
    }

    pub fn configure_command(&self, command: &mut std::process::Command) {
        for (key, value) in &self.environment {
            match value {
                Some(value) => {
                    command.env(key, value);
                }
                None => {
                    command.env_remove(key);
                }
            }
        }
    }

    pub(crate) fn write_denied(&self, path: &Path) -> bool {
        self.read_access(path) == Some(false)
            || codeterm_util::project_roots::canonical_target(path)
                .is_some_and(|target| self.shared_reads.iter().any(|grant| grant.allows(&target)))
    }

    pub(crate) fn read_access(&self, path: &Path) -> Option<bool> {
        let target = codeterm_util::project_roots::canonical_target(path)?;
        if self.isolation.denies(&target).unwrap_or(true) {
            return Some(false);
        }
        let under = |roots: &[PathBuf]| {
            roots.iter().any(|root| {
                codeterm_util::project_roots::canonical_target(root)
                    .is_some_and(|root| target.starts_with(root))
            })
        };
        if self
            .read_roots
            .iter()
            .zip(&self.pinned_read_roots)
            .any(|(root, pinned)| {
                target.starts_with(pinned)
                    && codeterm_util::project_roots::canonical_target(root).as_ref() == Some(pinned)
            })
        {
            return Some(true);
        }
        // The selected home's namespace exclusions protect shared targets, while
        // links inside that home may still resolve to a pinned shared grant.
        let selected_origin = self
            .read_roots
            .iter()
            .zip(&self.pinned_read_roots)
            .find(|(root, pinned)| {
                path.strip_prefix(root).is_ok_and(|relative| {
                    relative
                        .components()
                        .all(|part| matches!(part, std::path::Component::Normal(_)))
                }) && codeterm_util::project_roots::canonical_target(root).as_ref() == Some(*pinned)
            })
            .map(|(root, _)| root);
        if self.shared_exclusions.iter().any(|excluded| {
            target.starts_with(excluded)
                || (path.starts_with(excluded)
                    && !selected_origin.is_some_and(|root| root.starts_with(excluded)))
                || codeterm_util::project_roots::canonical_target(excluded)
                    .is_some_and(|excluded| target.starts_with(excluded))
        }) {
            return Some(false);
        }
        if self.shared_reads.iter().any(|grant| grant.allows(&target)) {
            return Some(true);
        }
        if self
            .shared_reads
            .iter()
            .any(|grant| grant.covers_path(path, &target))
            || self.read_roots.iter().any(|root| path.starts_with(root))
        {
            return Some(false);
        }
        let protected = self
            .protected_roots
            .iter()
            .any(|root| path.starts_with(root))
            || under(&self.protected_roots)
            || self.protected_roots.iter().any(|root| {
                codeterm_util::project_roots::canonical_target(root)
                    .is_some_and(|root| root.starts_with(&target))
            });
        protected.then_some(false)
    }
}

tokio::task_local! {
    static ASYNC_CONTEXT: Option<Arc<InvocationContext>>;
}
thread_local! {
    static SYNC_CONTEXT: RefCell<Option<Option<Arc<InvocationContext>>>> = const { RefCell::new(None) };
}

pub fn current() -> Option<Arc<InvocationContext>> {
    SYNC_CONTEXT
        .with(|slot| slot.borrow().clone())
        .unwrap_or_else(|| ASYNC_CONTEXT.try_with(Clone::clone).ok().flatten())
}

pub fn with_context<R>(context: Option<Arc<InvocationContext>>, work: impl FnOnce() -> R) -> R {
    struct Restore(Option<Option<Arc<InvocationContext>>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SYNC_CONTEXT.with(|slot| {
                slot.replace(self.0.take());
            });
        }
    }
    let _restore = Restore(SYNC_CONTEXT.with(|slot| slot.replace(Some(context))));
    work()
}

pub fn scope<F: Future>(
    context: Option<Arc<InvocationContext>>,
    work: F,
) -> impl Future<Output = F::Output> {
    let mut work = Box::pin(work);
    ASYNC_CONTEXT.scope(
        context.clone(),
        std::future::poll_fn(move |cx| with_context(context.clone(), || work.as_mut().poll(cx))),
    )
}

pub fn environment_is_selected(name: &str) -> bool {
    current().is_some_and(|context| {
        context
            .environment
            .keys()
            .any(|key| environment_name_matches(key, name))
    })
}

fn environment_name_matches(left: &str, right: &str) -> bool {
    left == right || (cfg!(windows) && left.eq_ignore_ascii_case(right))
}

pub fn environment(name: &str) -> Option<OsString> {
    if let Some(context) = current() {
        if let Some((_, value)) = context
            .environment
            .iter()
            .find(|(key, _)| environment_name_matches(key, name))
        {
            return value.as_ref().map(OsString::from);
        }
    }
    std::env::var_os(name)
}

pub(crate) fn read_allowed(path: &Path) -> bool {
    current().is_none_or(|context| context.read_access(path) != Some(false))
}

pub(crate) fn open_read(path: impl AsRef<Path>) -> std::io::Result<std::fs::File> {
    let path = path.as_ref();
    if !read_allowed(path) {
        return Err(std::io::ErrorKind::PermissionDenied.into());
    }
    std::fs::File::open(path)
}

pub(crate) struct AccountReadDir {
    entries: std::fs::ReadDir,
    context: Option<Arc<InvocationContext>>,
}

impl Iterator for AccountReadDir {
    type Item = std::io::Result<std::fs::DirEntry>;
    fn next(&mut self) -> Option<Self::Item> {
        self.entries.by_ref().find(|entry| {
            with_context(self.context.clone(), || {
                entry
                    .as_ref()
                    .map_or(true, |entry| read_allowed(&entry.path()))
            })
        })
    }
}

pub(crate) fn read_directory(path: impl AsRef<Path>) -> std::io::Result<AccountReadDir> {
    let path = path.as_ref();
    if !read_allowed(path) {
        return Err(std::io::ErrorKind::PermissionDenied.into());
    }
    Ok(AccountReadDir {
        entries: std::fs::read_dir(path)?,
        context: current(),
    })
}

pub(crate) fn read_to_string(path: impl AsRef<Path>) -> std::io::Result<String> {
    use std::io::Read;
    let mut contents = String::new();
    open_read(path)?.read_to_string(&mut contents)?;
    Ok(contents)
}

fn reserved_secret_name(name: &str) -> bool {
    name.split_once(':')
        .is_some_and(|(prefix, _)| prefix.eq_ignore_ascii_case("account"))
}

pub fn secret_name(plugin: &str, name: &str) -> Option<String> {
    if reserved_secret_name(name) {
        return None;
    }
    match current() {
        Some(context) => Some(format!(
            "{}{name}",
            context.credential_secret_prefix(plugin)?
        )),
        None => Some(name.to_owned()),
    }
}

pub fn secret_reference(
    provider: &str,
    reference: &str,
    context: Option<&InvocationContext>,
) -> Option<String> {
    let namespace = format!("domios:provider:{provider}:");
    let name = reference
        .strip_prefix(&namespace)
        .filter(|name| !name.is_empty() && !name.contains('\0') && !reserved_secret_name(name))?;
    match context {
        Some(context) => Some(format!(
            "{namespace}{}{name}",
            context.credential_secret_prefix(provider)?
        )),
        None => Some(reference.to_owned()),
    }
}

pub fn cache_key(provider: &str) -> String {
    current()
        .filter(|context| context.provider == provider)
        .map(|context| format!("{provider}@{}", context.cache_key))
        .unwrap_or_else(|| provider.to_owned())
}

#[cfg(test)]
mod tests;
