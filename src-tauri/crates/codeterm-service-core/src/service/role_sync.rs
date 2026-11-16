use super::{
    team_roles::{sync::Preview, TeamRoleStore},
    ServiceContext,
};
use codeterm_types::team::role_registration::RoleRegistration;
use serde_json::{json, Value};

const PREFIX: &str = "role-sync:";

fn source(team: &str, role: &str) -> Result<RoleRegistration, String> {
    super::builtin_roles::default_registrations(team)
        .into_iter()
        .find(|item| item.name == role && item.is_builtin_manager())
        .ok_or_else(|| "Role sync requires a built-in manager from the shipped default".into())
}

impl ServiceContext {
    fn role_sync_store(&self) -> &TeamRoleStore {
        #[cfg(test)]
        if let Some(store) = self.try_ext::<TeamRoleStore>() {
            return store;
        }
        TeamRoleStore::global()
    }

    fn role_sync_preview(&self, team: &str, role: &str) -> Result<Preview, String> {
        if self.mux.read().get_team(team).is_none() {
            return Err("Role sync requires a live local Team".into());
        }
        self.role_sync_store()
            .sync_preview(team, role, &source(team, role)?)
    }

    pub fn preview_role_sync(&self, team: &str, role: &str) -> Result<Value, String> {
        serde_json::to_value(self.role_sync_preview(team, role)?).map_err(|error| error.to_string())
    }

    pub fn request_role_sync(
        &self,
        team: &str,
        role: &str,
        current: &str,
        replacement: &str,
    ) -> Result<Value, String> {
        let preview = self.role_sync_preview(team, role)?;
        if preview.replacement_hash != replacement
            || (preview.current_hash != current && preview.current_hash != replacement)
        {
            return Err("Role sync hashes are stale; preview again".into());
        }
        if preview.current_hash == replacement {
            return Ok(json!({"state":"unchanged", "preview":preview}));
        }
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        let request = format!(
            "{PREFIX}{team}:{role}:{current}:{replacement}:{}",
            &nonce[..12]
        );
        let mut body = format!("Team {team}, role {role}\nSource: {}@{}\nCurrent SHA-256: {current}\nReplacement SHA-256: {replacement}\n{}\n\n", preview.source.name, preview.source.version, preview.takes_effect);
        if body.len() + preview.diff.len() <= 4096 {
            body.push_str(&preview.diff);
        } else {
            body.push_str(&format!("The full diff is available with `domios role sync {role} --team {team} --preview`. Sync approves exactly the two hashes above."));
        }
        let id = self.publish_daemon_question(
            request,
            format!("Sync {role} instructions for {team}?"),
            body,
            vec!["Sync".into(), "Cancel".into()],
        )?;
        Ok(json!({"state":"awaiting_owner", "notification_id":id, "preview":preview}))
    }

    pub(super) fn answer_role_sync(&self, request: &str, label: &str) -> Result<Value, String> {
        let fields: Vec<_> = request
            .strip_prefix(PREFIX)
            .ok_or("Invalid role sync request")?
            .split(':')
            .collect();
        let [team, role, current, replacement, _nonce] = fields.as_slice() else {
            return Err("Invalid role sync request".into());
        };
        if label == "Cancel" {
            return Ok(json!({"state":"cancelled"}));
        }
        if label != "Sync" {
            return Err("Invalid role sync choice".into());
        }
        if self.mux.read().get_team(team).is_none() {
            return Err("The Team is no longer live".into());
        }
        let changed = self.role_sync_store().sync_instructions(
            team,
            role,
            current,
            replacement,
            &source(team, role)?,
            super::team_core::now_ms(),
        )?;
        Ok(json!({"state":if changed {"synced"} else {"unchanged"}, "team_id":team, "role":role}))
    }
}

#[cfg(test)]
mod tests;
