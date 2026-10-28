use super::{Error, Permission, Value};

pub(super) fn merge(current: Permission, patch: Value) -> codeterm_neighbors::Result<Permission> {
    let patch = patch.as_object().ok_or(Error::InvalidRequest)?;
    let mut merged = serde_json::to_value(current).map_err(|_| Error::InvalidRequest)?;
    let fields = merged.as_object_mut().ok_or(Error::InvalidRequest)?;
    for (name, value) in patch {
        if !fields.contains_key(name) {
            return Err(Error::InvalidRequest);
        }
        fields.insert(name.clone(), value.clone());
    }
    let permission: Permission = serde_json::from_value(merged).map_err(|_| Error::InvalidRequest)?;
    permission.validate().map_err(|_| Error::InvalidRequest)?;
    Ok(permission)
}

#[cfg(test)]
mod tests;
