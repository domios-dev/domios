use std::sync::OnceLock;

use axum::Router;

use crate::service::extensions::ExtensionsBuilder;

#[derive(Clone)]
pub struct LocalApiRouter(pub Router);

pub type ExtensionRegistrar = fn(&mut ExtensionsBuilder);

static REGISTRAR: OnceLock<ExtensionRegistrar> = OnceLock::new();

pub fn register_rest_extensions(extensions: ExtensionRegistrar) {
    let _ = REGISTRAR.set(extensions);
}

pub(crate) fn register_extensions(builder: &mut ExtensionsBuilder) {
    if let Some(extensions) = REGISTRAR.get() {
        extensions(builder);
    }
}
