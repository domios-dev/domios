pub mod host;
pub mod local_recording;
pub mod receiver_target;
pub mod surface;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

extern crate domios_browser as codeterm_browser; // rename-bridge
#[cfg(target_os = "macos")]
extern crate domios_input_macos as codeterm_input_macos; // rename-bridge
#[cfg(windows)]
extern crate domios_input_windows as codeterm_input_windows; // rename-bridge
extern crate domios_local_recording as codeterm_local_recording; // rename-bridge
extern crate domios_paths as codeterm_paths; // rename-bridge
extern crate domios_plugin as codeterm_plugin; // rename-bridge
extern crate domios_util as codeterm_util; // rename-bridge
