use super::*;

pub(crate) fn area_allowed(areas: &[String], area: Option<&str>) -> bool {
    areas
        .iter()
        .any(|allowed| allowed == area.filter(|area| !area.is_empty()).unwrap_or("(none)"))
}

impl ServiceContext {
    pub async fn neighbor_memory_areas(
        &self,
        caller: &TabCaller,
    ) -> Result<Option<Vec<String>>, String> {
        let Some((_, permission)) = self.neighbor_capability_permission(caller).await? else {
            return Ok(None);
        };
        Capability::MemoryRead.require(&permission)?;
        Ok(permission.memory_areas)
    }

    pub(super) async fn authorize_neighbor_memory_operation(
        &self,
        permission: &Permission,
        wake_up: bool,
        item: Option<&str>,
    ) -> Result<(), String> {
        let Some(areas) = permission.memory_areas.as_ref() else {
            return Ok(());
        };
        if wake_up {
            return Err("neighbor_memory_area_denied: Restricted concierges must search their allowed areas instead of global wake-up".into());
        }
        if let Some(id) = item {
            let service = self.clone();
            let id = id.to_owned();
            let item = tokio::task::spawn_blocking(move || service.mem.get(&id))
                .await
                .map_err(|error| error.to_string())??;
            if item
                .as_ref()
                .is_some_and(|item| !area_allowed(areas, item.area.as_deref()))
            {
                return Err(
                    "neighbor_memory_area_denied: This item is outside the allowed memory areas"
                        .into(),
                );
            }
        }
        Ok(())
    }

    pub(crate) async fn validate_neighbor_memory_areas(
        &self,
        permission: &Permission,
    ) -> codeterm_neighbors::Result<()> {
        let Some(areas) = permission.memory_areas.clone() else {
            return Ok(());
        };
        let service = self.clone();
        let taxonomy = tokio::task::spawn_blocking(move || service.mem.get_taxonomy(None))
            .await
            .map_err(|_| codeterm_neighbors::Error::Gateway)?
            .map_err(|_| codeterm_neighbors::Error::Gateway)?;
        if areas.iter().any(|area| !taxonomy.contains_key(area)) {
            return Err(codeterm_neighbors::Error::MemoryAreaUnknown);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
