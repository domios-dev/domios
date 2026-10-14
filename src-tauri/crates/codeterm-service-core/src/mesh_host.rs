//! Mesh inventory wire views shared by the REST route and the desktop mesh commands.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::service::store::Service;
use codeterm_mesh::contract::inventory::{
    InventoryCapability, InventoryOperation, InventoryProvenance,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshInventoryResource {
    #[serde(flatten)]
    pub service: Service,
    pub resource_id: String,
    pub machine_id: String,
    pub machine_name: String,
    pub provenance: InventoryProvenance,
    pub capabilities: BTreeMap<InventoryOperation, InventoryCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshInventoryView {
    pub contract_version: u16,
    pub machine_id: String,
    pub machine_name: String,
    pub resources: Vec<MeshInventoryResource>,
    pub provenance: InventoryProvenance,
}
