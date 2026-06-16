//! `host.secret*` backend (host_api v2). A per-plugin keychain bucket, namespaced
//! by plugin id (`codeterm.plugin.<id>`) so a plugin sees only its own secrets —
//! never CodeTerm's global store or another plugin's bucket. Gated by the
//! `secrets` manifest permission (checked by the caller).

use crate::secrets::secret_file::SecretFile;

fn bucket(plugin_id: &str) -> SecretFile {
    SecretFile::new(&format!("codeterm.plugin.{plugin_id}"))
}

pub(super) fn get(plugin_id: &str, name: &str) -> Option<String> {
    bucket(plugin_id).get(name).ok()
}

pub(super) fn set(plugin_id: &str, name: &str, value: &str) -> bool {
    bucket(plugin_id).set(name, value).is_ok()
}

pub(super) fn delete(plugin_id: &str, name: &str) -> bool {
    bucket(plugin_id).delete(name).is_ok()
}
