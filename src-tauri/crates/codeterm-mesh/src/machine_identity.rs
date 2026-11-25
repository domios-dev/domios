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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct MachineIdentity {
    pub kind: MachineKind,
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

fn parse_saved(text: &str) -> Result<SavedIdentity, String> {
    let mut value: serde_json::Value =
        serde_json::from_str(text).map_err(|error| error.to_string())?;
    if let Some(identity) = value
        .get_mut("revision")
        .and_then(|revision| revision.get_mut("identity"))
        .and_then(serde_json::Value::as_object_mut)
    {
        identity.remove("color");
    }
    serde_json::from_value(value).map_err(|error| error.to_string())
}

fn state_file(path: &Path) -> StateFile {
    StateFile::new(path).with_validator(|text| parse_saved(text).map(|_| ()))
}

pub(crate) fn load(path: &Path, instance_id: &str) -> Option<Revision> {
    match state_file(path).load(parse_saved) {
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
pub struct SignedMachineIdentity {
    revision: Revision,
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
    fn message(instance_id: &str, revision: Revision) -> Option<Vec<u8>> {
        let mut message = b"domios-mesh-machine-identity-v1\0".to_vec();
        message.extend(serde_json::to_vec(&(instance_id, revision)).ok()?);
        Some(message)
    }

    pub(crate) fn sign(instance_id: &str, private_key: &str, revision: Revision) -> Option<Self> {
        let seed: [u8; 32] = STANDARD.decode(private_key).ok()?.try_into().ok()?;
        let signature = SigningKey::from_bytes(&seed).sign(&Self::message(instance_id, revision)?);
        Some(Self {
            revision,
            signature: STANDARD.encode(signature.to_bytes()),
        })
    }

    pub(crate) fn verify(&self, instance_id: &str, public_key: &str) -> Option<Revision> {
        if self.signature.len() != 88 {
            return None;
        }
        let key = crate::crown::verifying_key_from_b64(public_key).ok()?;
        let signature = Signature::from_slice(&STANDARD.decode(&self.signature).ok()?).ok()?;
        key.verify(&Self::message(instance_id, self.revision)?, &signature)
            .ok()?;
        Some(self.revision)
    }
}

#[cfg(test)]
mod tests;
