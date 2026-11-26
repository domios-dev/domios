use std::path::Path;

use base64::{engine::general_purpose::STANDARD, Engine};
use codeterm_util::persisted_state::{Loaded, StateFile};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum MachineKind {
    Laptop,
    Desktop,
    Server,
    Mac,
    Phone,
    Vm,
    Small,
    Other,
}

// Keep emitting a neutral wire color until 1.23.2 peers leave the fleet.
const LEGACY_WIRE_COLOR: &str = "graphite";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireMachineIdentity {
    kind: MachineKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ts_rs::TS)]
pub struct MachineIdentity {
    pub kind: MachineKind,
}

impl Serialize for MachineIdentity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        WireMachineIdentity {
            kind: self.kind,
            color: Some(LEGACY_WIRE_COLOR.to_owned()),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for MachineIdentity {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = WireMachineIdentity::deserialize(deserializer)?;
        Ok(Self { kind: wire.kind })
    }
}

impl MachineIdentity {
    pub(crate) fn inferred(os: &str, headless: bool) -> Self {
        Self {
            kind: MachineFacts::detect(os, headless).kind(),
        }
    }
}

struct MachineFacts<'a> {
    os: &'a str,
    virtualized: bool,
    small_device: bool,
    portable: bool,
    headless: bool,
}

impl<'a> MachineFacts<'a> {
    fn detect(os: &'a str, headless: bool) -> Self {
        let read = |path| {
            std::fs::read_to_string(path)
                .unwrap_or_default()
                .to_lowercase()
        };
        let hardware = if os == "linux" {
            read("/sys/class/dmi/id/product_name")
        } else {
            String::new()
        };
        let virtualized = [
            "kvm",
            "virtualbox",
            "vmware",
            "virtual machine",
            "qemu",
            "hvm domu",
            "amazon ec2",
            "google compute engine",
            "openstack",
        ]
        .iter()
        .any(|name| hardware.contains(name));
        let board = if os == "linux" {
            read("/proc/device-tree/model")
        } else {
            String::new()
        };
        let small_device = [
            "raspberry pi",
            "orange pi",
            "rock pi",
            "beaglebone",
            "banana pi",
        ]
        .iter()
        .any(|name| board.contains(name));
        let portable = (os == "windows" && windows_battery_present())
            || os == "linux"
                && (matches!(
                    read("/sys/class/dmi/id/chassis_type").trim(),
                    "8" | "9" | "10" | "14" | "30" | "31" | "32"
                ) || std::fs::read_dir("/sys/class/power_supply")
                    .into_iter()
                    .flatten()
                    .flatten()
                    .any(|entry| {
                        std::fs::read_to_string(entry.path().join("type"))
                            .is_ok_and(|kind| kind.trim() == "Battery")
                    }));
        Self {
            os,
            virtualized,
            small_device,
            portable,
            headless,
        }
    }

    fn kind(&self) -> MachineKind {
        match self.os {
            "macos" => MachineKind::Mac,
            "android" | "ios" => MachineKind::Phone,
            _ if self.virtualized => MachineKind::Vm,
            _ if self.small_device => MachineKind::Small,
            _ if self.portable => MachineKind::Laptop,
            "linux" if self.headless => MachineKind::Server,
            "linux" | "windows" => MachineKind::Desktop,
            _ => MachineKind::Other,
        }
    }
}

#[cfg(not(windows))]
fn windows_battery_present() -> bool {
    false
}

#[cfg(windows)]
fn windows_battery_present() -> bool {
    #[repr(C)]
    #[derive(Default)]
    struct SystemPowerStatus {
        ac_line: u8,
        battery_flag: u8,
        battery_life_percent: u8,
        system_status: u8,
        battery_life_time: u32,
        battery_full_life_time: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetSystemPowerStatus(status: *mut SystemPowerStatus) -> i32;
    }

    let mut status = SystemPowerStatus::default();
    // SAFETY: The pointer is writable and the structure uses the Win32 layout.
    let read = unsafe { GetSystemPowerStatus(&mut status) };
    read != 0 && status.battery_flag & 128 == 0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Revision {
    pub(super) identity: MachineIdentity,
    pub(super) number: u64,
}

#[derive(Serialize, Deserialize)]
struct SavedIdentity {
    instance_id: String,
    revision: Revision,
}

fn state_file(path: &Path) -> StateFile {
    StateFile::new(path).with_validator(|text| {
        serde_json::from_str::<SavedIdentity>(text)
            .map(|_| ())
            .map_err(|error| error.to_string())
    })
}

pub(crate) fn load(path: &Path, instance_id: &str) -> Option<Revision> {
    match state_file(path)
        .load(|text| serde_json::from_str::<SavedIdentity>(text).map_err(|error| error.to_string()))
    {
        Loaded::Parsed(saved) | Loaded::Recovered { value: saved, .. }
            if saved.instance_id == instance_id =>
        {
            Some(saved.revision)
        }
        _ => None,
    }
}

pub(crate) fn save(path: &Path, instance_id: &str, revision: Revision) -> Result<(), String> {
    let saved = SavedIdentity {
        instance_id: instance_id.to_owned(),
        revision,
    };
    state_file(path).save(&serde_json::to_string(&saved).map_err(|error| error.to_string())?)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRevision {
    identity: WireMachineIdentity,
    number: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedMachineIdentity {
    revision: WireRevision,
    signature: String,
}

pub(crate) fn deserialize_signed<'de, D>(
    deserializer: D,
) -> Result<Option<SignedMachineIdentity>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(serde_json::from_value(serde_json::Value::deserialize(deserializer)?).ok())
}

impl SignedMachineIdentity {
    fn message(instance_id: &str, revision: &WireRevision) -> Option<Vec<u8>> {
        let mut message = b"domios-mesh-machine-identity-v1\0".to_vec();
        message.extend(serde_json::to_vec(&(instance_id, revision)).ok()?);
        Some(message)
    }

    pub(crate) fn sign(instance_id: &str, private_key: &str, revision: Revision) -> Option<Self> {
        let wire = WireRevision {
            identity: WireMachineIdentity {
                kind: revision.identity.kind,
                color: Some(LEGACY_WIRE_COLOR.to_owned()),
            },
            number: revision.number,
        };
        let seed: [u8; 32] = STANDARD.decode(private_key).ok()?.try_into().ok()?;
        let signature = SigningKey::from_bytes(&seed).sign(&Self::message(instance_id, &wire)?);
        Some(Self {
            revision: wire,
            signature: STANDARD.encode(signature.to_bytes()),
        })
    }

    pub(crate) fn verify(&self, instance_id: &str, public_key: &str) -> Option<Revision> {
        if self.signature.len() != 88 {
            return None;
        }
        let key = crate::crown::verifying_key_from_b64(public_key).ok()?;
        let signature = Signature::from_slice(&STANDARD.decode(&self.signature).ok()?).ok()?;
        key.verify(&Self::message(instance_id, &self.revision)?, &signature)
            .ok()?;
        Some(Revision {
            identity: MachineIdentity {
                kind: self.revision.identity.kind,
            },
            number: self.revision.number,
        })
    }
}

#[cfg(test)]
mod tests;
