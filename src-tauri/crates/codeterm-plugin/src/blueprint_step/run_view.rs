use super::*;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunViewReference {
    pub reference: String,
    pub scope: RunScope,
    pub owner: String,
    pub blueprint: String,
    pub ordinal: u64,
    pub label: String,
}

impl OperationLedger {
    pub fn run_view(
        &self,
        scope: &RunScope,
        owner: &str,
        blueprint: &str,
        name: &str,
    ) -> Result<RunViewReference, String> {
        if [&scope.workspace, &scope.run, owner, blueprint]
            .into_iter()
            .any(|value| {
                value.is_empty() || value.len() > 128 || value.chars().any(char::is_control)
            })
        {
            return Err("invalid host run view identity".into());
        }
        let scope_key = serde_json::to_string(scope).map_err(|_| "invalid run scope")?;
        let blueprint_key = serde_json::to_string(&(&scope.workspace, &scope.team, blueprint))
            .map_err(|_| "invalid Blueprint identity")?;
        let mut db = self.inner.db.lock();
        let tx = db.transaction().map_err(view_error)?;
        if let Some(value) = tx
            .query_row(
                "SELECT record FROM blueprint_run_views WHERE scope=?1",
                [&scope_key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(view_error)?
        {
            let existing: RunViewReference =
                serde_json::from_str(&value).map_err(|_| "invalid stored run view")?;
            if existing.owner != owner || existing.blueprint != blueprint {
                return Err("run view authority changed".into());
            }
            return Ok(existing);
        }
        let ordinal: u64 = tx
            .query_row(
                "SELECT coalesce(max(ordinal),0)+1 FROM blueprint_run_views WHERE blueprint_key=?1",
                [&blueprint_key],
                |row| row.get(0),
            )
            .map_err(view_error)?;
        if ordinal > MAX_COST_AMOUNT {
            return Err("run ordinal exhausted".into());
        }
        let name: String = name
            .chars()
            .filter(|character| !character.is_control())
            .take(48)
            .collect();
        let view = RunViewReference {
            reference: format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            ),
            scope: scope.clone(),
            owner: owner.into(),
            blueprint: blueprint.into(),
            ordinal,
            label: format!(
                "{} run #{ordinal}",
                if name.is_empty() { "Blueprint" } else { &name }
            ),
        };
        tx.execute(
            "INSERT INTO blueprint_run_views VALUES(?1,?2,?3,?4,?5)",
            params![
                scope_key,
                blueprint_key,
                view.reference,
                ordinal,
                serde_json::to_string(&view).map_err(|_| "invalid run view")?
            ],
        )
        .map_err(view_error)?;
        tx.commit().map_err(view_error)?;
        Ok(view)
    }

    pub fn resolve_run_view(&self, reference: &str) -> Result<RunViewReference, String> {
        if reference.len() != 64 || !reference.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("invalid run view reference".into());
        }
        let value: Option<String> = self
            .inner
            .db
            .lock()
            .query_row(
                "SELECT record FROM blueprint_run_views WHERE reference=?1",
                [reference],
                |row| row.get(0),
            )
            .optional()
            .map_err(view_error)?;
        serde_json::from_str(&value.ok_or("run view unavailable")?)
            .map_err(|_| "invalid stored run view".into())
    }
}

fn view_error(_: rusqlite::Error) -> String {
    "run view persistence unavailable".into()
}
