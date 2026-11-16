use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    pub team_id: String,
    pub role: String,
    pub current_hash: String,
    pub replacement_hash: String,
    pub source: codeterm_types::team::role_registration::PinnedPackage,
    pub diff: String,
    pub takes_effect: &'static str,
}

pub(crate) fn hash(text: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(text.as_bytes()))
}

fn target<'a>(
    roles: &'a [RoleRegistration],
    role: &str,
    source: &RoleRegistration,
) -> Result<&'a RoleRegistration, String> {
    let current = roles
        .iter()
        .find(|item| item.name == role)
        .ok_or("Role sync requires an existing pinned registration")?;
    if !current.is_builtin_manager()
        || !source.is_builtin_manager()
        || current.name != source.name
        || current.team_id != source.team_id
        || current.package.name != source.package.name
    {
        return Err(
            "Role sync accepts only built-in managers pinned to the shipped default package".into(),
        );
    }
    Ok(current)
}

fn diff(before: &str, after: &str) -> String {
    if before == after {
        return String::new();
    }
    let old: Vec<_> = before.split_inclusive('\n').collect();
    let new: Vec<_> = after.split_inclusive('\n').collect();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let mut output = format!(
        "--- pinned\n+++ shipped\n@@ -{},{} +{},{} @@\n",
        prefix + 1,
        old.len() - prefix - suffix,
        prefix + 1,
        new.len() - prefix - suffix
    );
    for (sign, lines) in [
        ('-', &old[prefix..old.len() - suffix]),
        ('+', &new[prefix..new.len() - suffix]),
    ] {
        for line in lines {
            output.push(sign);
            output.push_str(line);
            if !line.ends_with('\n') {
                output.push_str("\n\\ No newline at end of file\n");
            }
        }
    }
    output
}

impl TeamRoleStore {
    pub(crate) fn sync_preview(
        &self,
        team: &str,
        role: &str,
        source: &RoleRegistration,
    ) -> Result<Preview, String> {
        valid_team_id(team)?;
        let _guard = self.cache.lock();
        let roles = self.load(team);
        let current = target(&roles, role, source)?;
        Ok(Preview {
            team_id: team.into(),
            role: role.into(),
            current_hash: hash(&current.instructions),
            replacement_hash: hash(&source.instructions),
            source: source.package.clone(),
            diff: diff(&current.instructions, &source.instructions),
            takes_effect: "Next fresh launch; running sessions keep their current instructions",
        })
    }

    pub(crate) fn sync_instructions(
        &self,
        team: &str,
        role: &str,
        current_hash: &str,
        replacement_hash: &str,
        source: &RoleRegistration,
        approved_at: u64,
    ) -> Result<bool, String> {
        self.sync_instructions_with(
            team,
            role,
            (current_hash, replacement_hash),
            source,
            approved_at,
            |file, text| file.save(text),
        )
    }

    fn sync_instructions_with(
        &self,
        team: &str,
        role: &str,
        hashes: (&str, &str),
        source: &RoleRegistration,
        approved_at: u64,
        save: impl FnOnce(&StateFile, &str) -> Result<(), String>,
    ) -> Result<bool, String> {
        let (current_hash, replacement_hash) = hashes;
        let file = self.file(team)?;
        let mut cache = self.cache.lock();
        let mut roles = self.load(team);
        let current = target(&roles, role, source)?;
        let replacement = hash(&source.instructions);
        if replacement != replacement_hash {
            return Err("Shipped instructions changed; preview again".into());
        }
        let current_digest = hash(&current.instructions);
        if current_digest == replacement {
            cache.insert(team.into(), roles);
            return Ok(false);
        }
        if current_digest != current_hash {
            return Err("Pinned instructions changed; preview again".into());
        }
        let decision = serde_json::json!({
            "team_id":team, "role":role, "current_hash":current_hash,
            "replacement_hash":replacement_hash, "source":source.package,
            "approved_at":approved_at, "decision":"sync_instructions"
        });
        let audit = StateFile::new(self.root.join(team).join(format!("role-sync-{role}.json")));
        audit.save(&serde_json::to_string_pretty(&decision).map_err(|error| error.to_string())?)?;
        let updated = roles
            .iter_mut()
            .find(|item| item.name == role)
            .ok_or("Role registration disappeared")?;
        updated.instructions.clone_from(&source.instructions);
        save(
            &file,
            &serde_json::to_string_pretty(&roles).map_err(|error| error.to_string())?,
        )?;
        cache.insert(team.into(), roles);
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
