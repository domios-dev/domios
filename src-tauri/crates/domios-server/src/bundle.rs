use include_dir::{include_dir, Dir};

include!(concat!(env!("OUT_DIR"), "/system_plugin_hashes.rs"));
static FILES: Dir<'static> = include_dir!("$OUT_DIR/bundled_plugins");

pub fn install() {
    codeterm_plugin::bundled::install(SYSTEM_PLUGIN_HASHES, &FILES);
}
