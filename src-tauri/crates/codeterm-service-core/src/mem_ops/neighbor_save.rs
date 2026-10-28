use crate::mem::store::{MemAuthor, MemKind, MemScope, NewMemItem};
use crate::multiplexer::types::NeighborAuthority;
use crate::service::{tab_credentials::TabCaller, ServiceContext};
use codeterm_neighbors::permissions::Permission;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct NeighborSaveBody {
    pub kind: String,
    pub area: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub importance: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    WriteNotAllowed,
    AreaNotGranted,
    KindNotAllowed,
    ContainsCredentials,
    TooLarge,
}

impl Refusal {
    pub fn code(self) -> &'static str {
        match self {
            Self::WriteNotAllowed => "neighbor_memory_write_not_allowed",
            Self::AreaNotGranted => "neighbor_memory_area_not_granted",
            Self::KindNotAllowed => "neighbor_memory_kind_not_allowed",
            Self::ContainsCredentials => "neighbor_memory_contains_credentials",
            Self::TooLarge => "neighbor_memory_too_large",
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: Neighbor memory save refused", self.code())
    }
}

pub fn validate(
    authority: Option<&NeighborAuthority>,
    permission: &Permission,
    body: NeighborSaveBody,
) -> Result<NewMemItem, Refusal> {
    if authority.is_none() || !permission.allowed || permission.memory_write_areas.is_empty() {
        return Err(Refusal::WriteNotAllowed);
    }
    if !permission.memory_write_areas.contains(&body.area) {
        return Err(Refusal::AreaNotGranted);
    }
    let kind = match body.kind.as_str() {
        "fact" => MemKind::Fact,
        "note" => MemKind::Note,
        _ => return Err(Refusal::KindNotAllowed),
    };
    let mut fields = std::iter::once(body.title.as_str())
        .chain(std::iter::once(body.body.as_str()))
        .chain(body.summary.as_deref())
        .chain(body.tags.iter().map(String::as_str));
    let total = fields
        .clone()
        .fold(body.area.len() + body.kind.len(), |size, text| {
            size.saturating_add(text.len())
        });
    if body.title.len() > 200 || body.body.len() > 16_384 || total > 32_752 {
        return Err(Refusal::TooLarge);
    }
    if fields.any(codeterm_blueprint_types::blueprint_memory::contains_credentials) {
        return Err(Refusal::ContainsCredentials);
    }
    Ok(NewMemItem {
        kind,
        scope: MemScope::Global,
        title: body.title,
        body: body.body,
        summary: body.summary,
        area: Some(body.area),
        tags: body.tags,
        source: None,
        source_path: None,
        importance: body
            .importance
            .filter(|value| value.is_finite())
            .unwrap_or(3.0)
            .clamp(1.0, 5.0),
        pinned: false,
    })
}

impl ServiceContext {
    pub async fn save_neighbor_memory(
        &self,
        caller: Option<TabCaller>,
        author: MemAuthor,
        body: NeighborSaveBody,
    ) -> Result<serde_json::Value, String> {
        if !author.is_neighbor() {
            return Err(Refusal::WriteNotAllowed.to_string());
        }
        let caller = caller.ok_or_else(|| Refusal::WriteNotAllowed.to_string())?;
        let (authority, permission) = self.neighbor_memory_write_grant(&caller).await?;
        if author.friendship_id.as_deref() != Some(authority.friendship_id.as_str()) {
            return Err(Refusal::WriteNotAllowed.to_string());
        }
        let item =
            validate(Some(&authority), &permission, body).map_err(|error| error.to_string())?;
        let service = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut context = crate::mem::backend::WriteContext::new(author, "neighbor");
            context.replace_guard = Some(crate::mem::store::ReplaceGuard::new(permission.memory_write_areas));
            service.mem.save_with_context(
                item,
                false,
                context,
            )
            .map(|outcome| serde_json::json!({"id":outcome.id}))
            .map_err(|error| match error {
                crate::mem::backend::SaveError::Guard(
                    crate::mem::store::ReplaceGuardError::AreaNotAllowed,
                ) => Refusal::AreaNotGranted.to_string(),
                crate::mem::backend::SaveError::Guard(
                    crate::mem::store::ReplaceGuardError::TitleConflict,
                ) => "neighbor_memory_title_conflict: Neighbor memory save refused".into(),
                crate::mem::backend::SaveError::Storage(error) => error,
            })
        })
        .await
        .map_err(|error| error.to_string())?
    }
}

#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "test-support"))]
pub mod matrix;
