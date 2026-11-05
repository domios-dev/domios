use codeterm_plugin::agent_runtime::{install_validation, RuntimeLaunchProfile};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

const INDEX: &str = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";
const MAX_BYTES: usize = 2 * 1024 * 1024;

#[derive(Deserialize)]
struct Index {
    version: String,
    agents: Vec<Agent>,
}
#[derive(Deserialize)]
struct Agent {
    id: String,
    name: String,
    version: String,
    description: String,
    #[serde(default)]
    icon: Option<String>,
    distribution: Distribution,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Distribution {
    npx: Option<Package>,
    uvx: Option<Package>,
    binary: Option<BTreeMap<String, Binary>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Package {
    package: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binary {
    archive: String,
    cmd: String,
    sha256: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

fn decode(text: &str) -> Result<Index, String> {
    if text.len() > MAX_BYTES {
        return Err("Oversized ACP registry".into());
    }
    let index: Index = serde_json::from_str(text).map_err(|_| "Invalid ACP registry")?;
    if index.version != "1.0.0" || index.agents.len() > 512 {
        return Err("Unsupported or oversized ACP registry".into());
    }
    let mut seen = BTreeSet::new();
    for agent in &index.agents {
        if agent.id.len() > 128
            || !agent.id.starts_with(|c: char| c.is_ascii_lowercase())
            || !agent
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || !seen.insert(&agent.id)
            || semver::Version::parse(&agent.version).is_err()
            || agent.name.len() > 256
            || agent.description.len() > 4096
            || agent
                .name
                .chars()
                .chain(agent.description.chars())
                .any(char::is_control)
            || agent
                .icon
                .as_deref()
                .is_some_and(|url| !install_validation::https(url))
        {
            return Err("Invalid ACP registry entry".into());
        }
    }
    Ok(index)
}

fn fetch() -> Result<Index, String> {
    crate::async_runtime::block_on(async {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .map_err(|_| "Registry client unavailable")?;
        let mut response = client
            .get(INDEX)
            .send()
            .await
            .map_err(|_| "ACP registry unavailable")?
            .error_for_status()
            .map_err(|_| "ACP registry unavailable")?;
        if response
            .content_length()
            .is_some_and(|len| len > MAX_BYTES as u64)
        {
            return Err("Oversized ACP registry".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "ACP registry unavailable")?
        {
            if chunk.len() > MAX_BYTES.saturating_sub(bytes.len()) {
                return Err("Oversized ACP registry".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| "Invalid ACP registry encoding")?;
        decode(text)
    })
}

pub(super) fn search(query: &str) -> Result<Value, String> {
    if query.len() > 256 {
        return Err("Registry query exceeds size limit".into());
    }
    let needle = query.trim().to_ascii_lowercase();
    Ok(Value::Array(fetch()?.agents.into_iter().filter(|agent| {
        [&agent.id,&agent.name,&agent.description].iter().any(|text| text.to_ascii_lowercase().contains(&needle))
    }).map(|agent| json!({"id":agent.id,"name":agent.name,"version":agent.version,"description":agent.description,"iconUrl":agent.icon})).collect()))
}

fn platform() -> Result<String, String> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "windows",
        "linux" => "linux",
        _ => return Err("Unsupported registry platform".into()),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        _ => return Err("Unsupported registry architecture".into()),
    };
    Ok(format!("{os}-{arch}"))
}

pub(super) fn profile(id: &str) -> Result<RuntimeLaunchProfile, String> {
    mapped(&fetch()?, id, &platform()?)
}

fn mapped(index: &Index, id: &str, target: &str) -> Result<RuntimeLaunchProfile, String> {
    let agent = index
        .agents
        .iter()
        .find(|agent| agent.id == id)
        .ok_or("Unknown ACP registry agent")?;
    let mut value = json!({"ownerConfigured":true,"registryId":agent.id,"iconUrl":agent.icon});
    if let Some(package) = &agent.distribution.npx {
        let pin = if install_validation::exact_npm(&package.package) {
            let (_, version) = package
                .package
                .rsplit_once('@')
                .ok_or("Invalid package pin")?;
            if version != agent.version {
                return Err("Registry package version disagrees with the agent version".into());
            }
            package.package.clone()
        } else {
            format!("{}@{}", package.package, agent.version)
        };
        value["binary"] = "npx".into();
        value["args"] = json!(std::iter::once(pin.clone())
            .chain(package.args.clone())
            .collect::<Vec<_>>());
        value["env"] = json!(package.env);
        value["install"] = json!({"distribution":"npx","package":pin});
    } else if let Some(package) = &agent.distribution.uvx {
        let pin = if let Some((_, version)) = install_validation::exact_python(&package.package) {
            if version != agent.version {
                return Err("Registry package version disagrees with the agent version".into());
            }
            package.package.clone()
        } else {
            format!("{}=={}", package.package, agent.version)
        };
        value["binary"] = "uvx".into();
        value["args"] = json!(std::iter::once(pin.clone())
            .chain(package.args.clone())
            .collect::<Vec<_>>());
        value["env"] = json!(package.env);
        value["install"] = json!({"distribution":"uvx","package":pin});
    } else {
        let binary = agent
            .distribution
            .binary
            .as_ref()
            .and_then(|entries| entries.get(target))
            .ok_or("Registry has no distribution for this platform")?;
        value["binary"] = json!(binary.cmd);
        value["args"] = json!(binary.args);
        value["env"] = json!(binary.env);
        value["install"] = json!({"distribution":"binary","package":format!("{}@{}",agent.id,agent.version),"url":binary.archive,"executable":binary.cmd,"sha256":binary.sha256});
    }
    let profile: RuntimeLaunchProfile =
        serde_json::from_value(value).map_err(|_| "Invalid registry distribution")?;
    if !install_validation::validate(&profile) {
        return Err(
            "Registry distribution requires an exact package pin or a binary sha256".into(),
        );
    }
    let mut seen = BTreeSet::new();
    super::entries(
        profile
            .env
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect(),
        &mut seen,
    )?;
    Ok(profile)
}

#[cfg(test)]
mod tests;
