//! Host-rendered tab menu contributions declared by plugin manifests.

use std::sync::Arc;

use crate::capability::CapabilityRegistry;

pub use crate::js_runtime::manifest::{TabMenuAction, TabMenuItem, MAX_TAB_MENU_ITEMS};

pub const TAB_MENU_CONTRIBUTION_KIND: &str = "tab_menu";

pub type TabMenuRegistry = CapabilityRegistry<Vec<TabMenuItem>>;
pub type TabMenuRegistryHandle = Arc<TabMenuRegistry>;
